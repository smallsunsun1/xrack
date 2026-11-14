use std::{
    ffi::{CStr, CString, c_int, c_void},
    fmt,
    path::Path,
    ptr,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Instant,
};

use crate::{
    ff,
    native::{api, load},
};

#[derive(Debug)]
pub struct MediaError(pub(crate) String);

impl fmt::Display for MediaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for MediaError {}

pub(crate) fn check(code: i32, stage: &str) -> Result<(), MediaError> {
    if code >= 0 {
        return Ok(());
    }
    let mut buffer = [0_i8; 192];
    // FFmpeg writes a terminated string on success.
    let message = unsafe {
        if (api().av_strerror.unwrap())(code, buffer.as_mut_ptr().cast(), buffer.len() as _) == 0 {
            std::ffi::CStr::from_ptr(buffer.as_ptr().cast())
                .to_string_lossy()
                .into_owned()
        } else {
            format!("FFmpeg error {code}")
        }
    };
    Err(MediaError(format!("{stage}: {message}")))
}

#[derive(Debug)]
pub struct VideoFrame {
    pub size: [u32; 2],
    pub rgba: Vec<u8>,
    pub timestamp_seconds: Option<f64>,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct LiveWindow {
    pub start: f64,
    pub end: f64,
    pub edge: f64,
}

impl LiveWindow {
    pub(crate) fn clamp(self, position: f64) -> f64 {
        position.clamp(self.start, (self.end - 0.15).max(self.start))
    }

    pub(crate) fn intersect(self, other: Self) -> Option<Self> {
        let start = self.start.max(other.start);
        let end = self.end.min(other.end);
        (end > start).then(|| Self {
            start,
            end,
            edge: self.edge.max(other.edge).clamp(start, end),
        })
    }
}

pub(crate) struct Interrupt {
    pub cancelled: Arc<AtomicBool>,
    origin: Instant,
    deadline_ms: AtomicU64,
}

impl Interrupt {
    fn new(cancelled: Arc<AtomicBool>) -> Self {
        Self {
            cancelled,
            origin: Instant::now(),
            deadline_ms: AtomicU64::new(0),
        }
    }

    fn arm(&self) {
        self.deadline_ms
            .store(self.elapsed_ms() + 8_000, Ordering::Relaxed);
    }

    fn elapsed_ms(&self) -> u64 {
        self.origin.elapsed().as_millis() as u64
    }
}

unsafe extern "C" fn interrupt(opaque: *mut c_void) -> c_int {
    let io = unsafe { &*opaque.cast::<Interrupt>() };
    let deadline = io.deadline_ms.load(Ordering::Relaxed);
    i32::from(io.cancelled.load(Ordering::Relaxed) || deadline > 0 && io.elapsed_ms() >= deadline)
}

pub(crate) fn is_network(path: &Path) -> bool {
    path.to_str()
        .is_some_and(|source| source.starts_with("https://") || source.starts_with("http://"))
}

struct Options(*mut ff::AVDictionary);

impl Options {
    fn set(&mut self, key: &CStr, value: &CStr) -> Result<(), MediaError> {
        check(
            unsafe { (api().av_dict_set.unwrap())(&mut self.0, key.as_ptr(), value.as_ptr(), 0) },
            "configure media input",
        )
    }
}

impl Drop for Options {
    fn drop(&mut self) {
        unsafe { (api().av_dict_free.unwrap())(&mut self.0) }
    }
}

pub struct VideoDecoder {
    format: *mut ff::AVFormatContext,
    codec: *mut ff::AVCodecContext,
    frame: *mut ff::AVFrame,
    packet: *mut ff::AVPacket,
    scale: *mut ff::SwsContext,
    stream: i32,
    time_base: ff::AVRational,
    origin: f64,
    duration: f64,
    draining: bool,
    io: Box<Interrupt>,
    seekable: bool,
    hls: bool,
    live: bool,
    variant: usize,
    variant_count: usize,
    variant_bitrate: i64,
    audio_tracks: i64,
    subtitle_tracks: i64,
    title: String,
    hls_seek_target: Option<f64>,
    packet_bytes: u64,
}

impl VideoDecoder {
    pub fn open(path: &Path) -> Result<Self, MediaError> {
        Self::open_video(path, Arc::default())?.ok_or_else(|| MediaError("no video stream".into()))
    }

    pub(crate) fn open_video(
        path: &Path,
        cancelled: Arc<AtomicBool>,
    ) -> Result<Option<Self>, MediaError> {
        Self::open_video_variant(path, 0, cancelled)
    }

    pub(crate) fn open_video_variant(
        path: &Path,
        variant: usize,
        cancelled: Arc<AtomicBool>,
    ) -> Result<Option<Self>, MediaError> {
        Self::open_stream(
            path,
            ff::AVMediaType_AVMEDIA_TYPE_VIDEO,
            -1,
            variant,
            cancelled,
        )
    }

    #[cfg(test)]
    pub(crate) fn open_audio(path: &Path) -> Result<Option<Self>, MediaError> {
        Self::open_audio_track(path, -1, 0, Arc::default())
    }

    pub(crate) fn open_audio_track(
        path: &Path,
        track: i64,
        variant: usize,
        cancelled: Arc<AtomicBool>,
    ) -> Result<Option<Self>, MediaError> {
        Self::open_stream(
            path,
            ff::AVMediaType_AVMEDIA_TYPE_AUDIO,
            track,
            variant,
            cancelled,
        )
    }

    pub(crate) fn open_subtitle_track(
        path: &Path,
        track: i64,
        variant: usize,
        cancelled: Arc<AtomicBool>,
    ) -> Result<Option<Self>, MediaError> {
        Self::open_stream(
            path,
            ff::AVMediaType_AVMEDIA_TYPE_SUBTITLE,
            track,
            variant,
            cancelled,
        )
    }

    fn open_stream(
        path: &Path,
        media_type: ff::AVMediaType,
        track: i64,
        variant: usize,
        cancelled: Arc<AtomicBool>,
    ) -> Result<Option<Self>, MediaError> {
        let network = is_network(path);
        let local;
        let path = if network {
            path
        } else {
            local = path
                .canonicalize()
                .map_err(|error| MediaError(error.to_string()))?;
            if !local.is_file() {
                return Err(MediaError(
                    "media input must be a local file or HTTP(S) URL".into(),
                ));
            }
            &local
        };
        let path = path
            .to_str()
            .ok_or_else(|| MediaError("media path must be UTF-8".into()))?;
        let path = CString::new(path).map_err(|error| MediaError(error.to_string()))?;
        load().map_err(|error| MediaError(error.into()))?;
        let mut options = Options(ptr::null_mut());
        if network {
            // Permit remote HLS segments and keys, while rejecting local-file
            // references and HTTPS-to-HTTP redirects.
            let protocols = if path.as_bytes().starts_with(b"https://") {
                c"https,tcp,tls,crypto"
            } else {
                c"http,https,tcp,tls,crypto"
            };
            options.set(c"protocol_whitelist", protocols)?;
            options.set(c"rw_timeout", c"8000000")?;
            options.set(c"tls_verify", c"1")?;
            options.set(c"reconnect", c"1")?;
            options.set(c"reconnect_on_network_error", c"1")?;
            options.set(c"reconnect_streamed", c"1")?;
            options.set(c"reconnect_max_retries", c"2")?;
            options.set(c"reconnect_delay_total_max", c"4")?;
            if let Some(path) = [
                std::env::var_os("SSL_CERT_FILE").map(std::path::PathBuf::from),
                Some("/etc/ssl/certs/ca-certificates.crt".into()),
                Some("/etc/ssl/cert.pem".into()),
            ]
            .into_iter()
            .flatten()
            .find(|path| path.is_file())
                && let Ok(value) = CString::new(path.to_string_lossy().as_bytes())
            {
                options.set(c"ca_file", &value)?;
            }
        } else {
            options.set(c"protocol_whitelist", c"file,crypto")?;
        }
        let mut decoder = Self {
            format: ptr::null_mut(),
            codec: ptr::null_mut(),
            frame: ptr::null_mut(),
            packet: ptr::null_mut(),
            scale: ptr::null_mut(),
            stream: -1,
            time_base: ff::AVRational { num: 0, den: 1 },
            origin: 0.0,
            duration: 0.0,
            draining: false,
            io: Box::new(Interrupt::new(cancelled)),
            seekable: !network,
            hls: false,
            live: false,
            variant: 0,
            variant_count: 0,
            variant_bitrate: 0,
            audio_tracks: 0,
            subtitle_tracks: 0,
            title: String::new(),
            hls_seek_target: None,
            packet_bytes: 0,
        };

        // All allocations are owned by the decoder immediately, including failed opens.
        unsafe {
            decoder.format = (api().avformat_alloc_context.unwrap())();
            if decoder.format.is_null() {
                return Err(MediaError("allocate media format".into()));
            }
            let context = decoder.format.cast::<u8>();
            context
                .add(ff::XRACK_FFMPEG_INTERRUPT_OFFSET as usize)
                .cast::<ff::AVIOInterruptCB>()
                .write(ff::AVIOInterruptCB {
                    callback: Some(interrupt),
                    opaque: (&mut *decoder.io as *mut Interrupt).cast(),
                });
            decoder.io.arm();
            check(
                (api().avformat_open_input.unwrap())(
                    &mut decoder.format,
                    path.as_ptr(),
                    ptr::null(),
                    &mut options.0,
                ),
                "open media",
            )?;
            decoder.io.arm();
            check(
                (api().avformat_find_stream_info.unwrap())(decoder.format, ptr::null_mut()),
                "read stream metadata",
            )?;
            let format = decoder.format.cast::<u8>();
            let count = *format
                .add(ff::XRACK_FFMPEG_STREAM_COUNT_OFFSET as usize)
                .cast::<u32>();
            let streams = *format
                .add(ff::XRACK_FFMPEG_STREAMS_OFFSET as usize)
                .cast::<*mut *mut ff::AVStream>();
            let input = *format
                .add(ff::XRACK_FFMPEG_FORMAT_OFFSET as usize)
                .cast::<*const ff::AVInputFormat>();
            decoder.hls = input
                .as_ref()
                .is_some_and(|input| CStr::from_ptr(input.name) == c"hls");
            let program = if decoder.hls {
                let n = *format
                    .add(ff::XRACK_FFMPEG_PROGRAM_COUNT_OFFSET as usize)
                    .cast::<u32>();
                let programs = *format
                    .add(ff::XRACK_FFMPEG_PROGRAMS_OFFSET as usize)
                    .cast::<*mut *mut ff::AVProgram>();
                let mut variants = (0..n as usize)
                    .map(|i| *programs.add(i))
                    .filter(|program| !program.is_null())
                    .filter_map(|program| {
                        let bitrate = (api().av_dict_get.unwrap())(
                            (*program).metadata,
                            c"variant_bitrate".as_ptr(),
                            ptr::null(),
                            0,
                        )
                        .as_ref()?;
                        let bitrate: i64 =
                            CStr::from_ptr(bitrate.value).to_str().ok()?.parse().ok()?;
                        (bitrate > 0).then_some((bitrate, program))
                    })
                    .collect::<Vec<_>>();
                variants.sort_by_key(|(bitrate, _)| *bitrate);
                decoder.variant_count = variants.len();
                decoder.variant = variant.min(variants.len().saturating_sub(1));
                variants.get(decoder.variant).map(|&(bitrate, program)| {
                    decoder.variant_bitrate = bitrate;
                    for &(_, other) in &variants {
                        if other != program {
                            (*other).discard = ff::AVDiscard_AVDISCARD_ALL;
                        }
                    }
                    let count = (*program).nb_stream_indexes as usize;
                    if count == 0 {
                        &[][..]
                    } else {
                        std::slice::from_raw_parts((*program).stream_index, count)
                    }
                })
            } else {
                None
            };
            let tracks = |kind| {
                (0..count as usize)
                    .filter(|index| {
                        program.is_none_or(|streams| streams.contains(&(*index as u32)))
                    })
                    .filter(|index| (*(*(*streams.add(*index))).codecpar).codec_type == kind)
                    .collect::<Vec<_>>()
            };
            let audio_streams = tracks(ff::AVMediaType_AVMEDIA_TYPE_AUDIO);
            let subtitles = tracks(ff::AVMediaType_AVMEDIA_TYPE_SUBTITLE);
            decoder.audio_tracks = audio_streams.len() as i64;
            decoder.subtitle_tracks = subtitles.len() as i64;
            if network {
                let pb = *format
                    .add(ff::XRACK_FFMPEG_POINTER_OFFSET as usize)
                    .cast::<*mut ff::AVIOContext>();
                decoder.seekable = pb
                    .as_ref()
                    .is_some_and(|pb| pb.seekable & ff::XRACK_FFMPEG_SEEKABLE_NORMAL as i32 != 0);
            }
            let metadata = *format
                .add(ff::XRACK_FFMPEG_METADATA_OFFSET as usize)
                .cast::<*mut ff::AVDictionary>();
            if let Some(title) =
                (api().av_dict_get.unwrap())(metadata, c"title".as_ptr(), ptr::null(), 0).as_ref()
            {
                decoder.title = CStr::from_ptr(title.value).to_string_lossy().into_owned();
            }
            let mut codec = ptr::null();
            if track >= 0 || program.is_some() {
                let streams_of_type: &[usize] = if media_type == ff::AVMediaType_AVMEDIA_TYPE_AUDIO
                {
                    &audio_streams
                } else if media_type == ff::AVMediaType_AVMEDIA_TYPE_SUBTITLE {
                    &subtitles
                } else {
                    decoder.stream = tracks(media_type).first().map_or(-1, |index| *index as i32);
                    &[]
                };
                if media_type != ff::AVMediaType_AVMEDIA_TYPE_VIDEO {
                    let Some(&index) = streams_of_type.get(track.max(0) as usize) else {
                        return if track < 0 {
                            Ok(None)
                        } else {
                            Err(MediaError(format!("media track {track} is unavailable")))
                        };
                    };
                    decoder.stream = index as i32;
                }
                if decoder.stream < 0 {
                    return Ok(None);
                }
                codec = (api().avcodec_find_decoder.unwrap())(
                    (*(*(*streams.add(decoder.stream as usize))).codecpar).codec_id,
                );
                if codec.is_null() {
                    return Err(MediaError("unsupported media track codec".into()));
                }
            } else {
                decoder.stream = (api().av_find_best_stream.unwrap())(
                    decoder.format,
                    media_type,
                    -1,
                    -1,
                    &mut codec,
                    0,
                );
            }
            if decoder.stream == ff::XRACK_FFMPEG_STREAM_NOT_FOUND {
                return Ok(None);
            }
            check(decoder.stream, "find media stream")?;
            let format = decoder.format.cast::<u8>();
            let start = *format
                .add(ff::XRACK_FFMPEG_START_OFFSET as usize)
                .cast::<i64>();
            let duration = *format
                .add(ff::XRACK_FFMPEG_DURATION_OFFSET as usize)
                .cast::<i64>();
            decoder.origin = if start == i64::MIN {
                0.0
            } else {
                start as f64 / 1_000_000.0
            };
            decoder.duration = duration.max(0) as f64 / 1_000_000.0;
            if decoder.hls {
                let flags = *format
                    .add(ff::XRACK_FFMPEG_CONTEXT_FLAGS_OFFSET as usize)
                    .cast::<i32>();
                decoder.seekable = flags & ff::XRACK_FFMPEG_UNSEEKABLE == 0;
                // EVENT playlists can advertise a duration before they end.
                let mut start = 0;
                let mut end = 0;
                let mut edge = 0;
                let result = (api().xr_hls_window.unwrap())(
                    decoder.format,
                    decoder.stream,
                    0,
                    &mut start,
                    &mut end,
                    &mut edge,
                );
                decoder.live = result != ff::XRACK_FFMPEG_NOT_IMPLEMENTED;
                if decoder.live {
                    decoder.duration = 0.0;
                }
            }
            decoder.codec = ((api().avcodec_alloc_context3).unwrap())(codec);
            decoder.frame = ((api().av_frame_alloc).unwrap())();
            decoder.packet = ((api().av_packet_alloc).unwrap())();
            if decoder.codec.is_null() || decoder.frame.is_null() || decoder.packet.is_null() {
                return Err(MediaError("allocate video decoder".into()));
            }
            // The offset comes from the FFmpeg headers. The stream index was
            // validated by av_find_best_stream and the format owns the array.
            let stream = &**streams.add(decoder.stream as usize);
            decoder.time_base = stream.time_base;
            (*decoder.codec).pkt_timebase = decoder.time_base;
            check(
                ((api().avcodec_parameters_to_context).unwrap())(decoder.codec, stream.codecpar),
                "copy stream parameters",
            )?;
            check(
                ((api().avcodec_open2).unwrap())(decoder.codec, codec, ptr::null_mut()),
                "open video codec",
            )?;
        }
        Ok(Some(decoder))
    }

    pub fn duration(&self) -> f64 {
        self.duration
    }

    pub(crate) fn seekable(&self) -> bool {
        self.live_window(false).is_some() || self.seekable && self.duration > 0.0
    }

    pub(crate) fn is_live(&self) -> bool {
        self.live
    }

    pub(crate) fn take_packet_bytes(&mut self) -> u64 {
        std::mem::take(&mut self.packet_bytes)
    }

    pub(crate) fn origin(&self) -> f64 {
        self.origin
    }

    pub(crate) fn set_live_origin(&mut self, origin: f64) {
        if self.live {
            self.origin = origin;
        }
    }

    pub(crate) fn live_window(&self, refresh: bool) -> Option<LiveWindow> {
        if !self.live {
            return None;
        }
        if refresh {
            self.io.arm();
        }
        let (mut start, mut end, mut edge) = (0, 0, 0);
        // Only the decoder worker calls this. FFmpeg owns the demuxer and its playlists.
        let result = unsafe {
            (api().xr_hls_window.unwrap())(
                self.format,
                self.stream,
                i32::from(refresh),
                &mut start,
                &mut end,
                &mut edge,
            )
        };
        (result == 0 && end > start).then(|| LiveWindow {
            start: start as f64 / 1_000_000.0 - self.origin,
            end: end as f64 / 1_000_000.0 - self.origin,
            edge: edge as f64 / 1_000_000.0 - self.origin,
        })
    }
    pub(crate) fn variants(&self) -> (usize, usize, i64) {
        (self.variant, self.variant_count, self.variant_bitrate)
    }
    pub(crate) fn audio_tracks(&self) -> i64 {
        self.audio_tracks
    }
    pub(crate) fn title(&self) -> &str {
        &self.title
    }

    pub(crate) fn subtitle_tracks(&self) -> i64 {
        self.subtitle_tracks
    }

    pub(crate) fn next_cue(&mut self) -> Result<Option<crate::subtitles::Cue>, MediaError> {
        use crate::subtitles::Cue;
        unsafe {
            loop {
                self.io.arm();
                let read = (api().av_read_frame.unwrap())(self.format, self.packet);
                if read == ff::XRACK_FFMPEG_EOF {
                    return Ok(None);
                }
                check(read, "read subtitle packet")?;
                if (*self.packet).stream_index != self.stream {
                    (api().av_packet_unref.unwrap())(self.packet);
                    continue;
                }
                let base = self.time_base;
                let pts = (*self.packet).pts;
                let packet_duration = (*self.packet).duration;
                let fallback_duration = (packet_duration > 0 && base.den > 0)
                    .then(|| packet_duration as f64 * f64::from(base.num) / f64::from(base.den));
                let fallback = (pts != i64::MIN && base.den > 0)
                    .then(|| pts as f64 * f64::from(base.num) / f64::from(base.den));
                let mut subtitle: ff::AVSubtitle = std::mem::zeroed();
                let mut decoded = 0;
                // FFmpeg text subtitle codecs use the legacy subtitle API.
                let result = (api().avcodec_decode_subtitle2.unwrap())(
                    self.codec,
                    &mut subtitle,
                    &mut decoded,
                    self.packet,
                );
                (api().av_packet_unref.unwrap())(self.packet);
                if result < 0 || decoded == 0 {
                    (api().avsubtitle_free.unwrap())(&mut subtitle);
                    check(result, "decode text subtitle")?;
                    continue;
                }
                let time = if subtitle.pts == i64::MIN {
                    fallback.unwrap_or(self.origin)
                } else {
                    subtitle.pts as f64 / 1_000_000.0
                };
                let time = time - self.origin;
                let start = (time + f64::from(subtitle.start_display_time) / 1_000.0).max(0.0);
                let end = if subtitle.end_display_time > subtitle.start_display_time {
                    time + f64::from(subtitle.end_display_time) / 1_000.0
                } else {
                    start + fallback_duration.unwrap_or(0.0)
                };
                let mut lines = Vec::new();
                for index in 0..subtitle.num_rects as usize {
                    let rect = &**subtitle.rects.add(index);
                    if !rect.text.is_null() {
                        lines.push(CStr::from_ptr(rect.text).to_string_lossy().into_owned());
                    } else if !rect.ass.is_null() {
                        let ass = CStr::from_ptr(rect.ass).to_string_lossy();
                        if let Some(text) = ass.splitn(9, ',').nth(8) {
                            // ASS presentation tags are not a part of the text.
                            let mut clean = String::new();
                            let mut tag = false;
                            for token in text.chars() {
                                match token {
                                    '{' => tag = true,
                                    '}' => tag = false,
                                    character if !tag => clean.push(character),
                                    _ => {}
                                }
                            }
                            lines.push(clean.replace("\\N", "\n").replace("\\n", "\n"));
                        }
                    }
                }
                (api().avsubtitle_free.unwrap())(&mut subtitle);
                let text = lines.join("\n");
                if end > start && !text.is_empty() {
                    return Ok(Some(Cue { start, end, text }));
                }
            }
        }
    }

    pub fn seek(&mut self, position: f64) -> Result<(), MediaError> {
        if !position.is_finite() {
            return Err(MediaError("playback position must be finite".into()));
        }
        // HLS packet timestamps can start before the presentation clock.
        let (position, target) = if self.live {
            let window = self
                .live_window(false)
                .ok_or_else(|| MediaError("live seek window is unavailable".into()))?;
            let position = window.clamp(position);
            (position, (position - 1.0).max(window.start))
        } else if self.hls {
            (position, (position - 1.0).max(0.0))
        } else {
            (position, position)
        };
        self.io.arm();
        unsafe {
            let timestamp = target + self.origin;
            let (stream, timestamp) = if self.hls {
                (
                    self.stream,
                    timestamp * f64::from(self.time_base.den) / f64::from(self.time_base.num),
                )
            } else {
                (-1, timestamp * 1_000_000.0)
            };
            let mut flags = ff::XRACK_FFMPEG_SEEK_BACKWARD;
            if self.hls && (*self.codec).codec_type != ff::AVMediaType_AVMEDIA_TYPE_VIDEO {
                flags |= ff::XRACK_FFMPEG_SEEK_ANY;
            }
            check(
                (api().av_seek_frame.unwrap())(self.format, stream, timestamp as i64, flags),
                "seek media",
            )?;
            (api().avcodec_flush_buffers.unwrap())(self.codec);
        }
        self.hls_seek_target = self.hls.then_some(position);
        self.draining = false;
        self.packet_bytes = 0;
        Ok(())
    }

    pub fn next_frame(&mut self) -> Result<Option<VideoFrame>, MediaError> {
        self.receive()?.then(|| self.convert()).transpose()
    }

    pub(crate) fn frame(&self) -> &ff::AVFrame {
        unsafe { &*self.frame }
    }

    pub(crate) fn timestamp(&self) -> Option<f64> {
        let pts = self.frame().best_effort_timestamp;
        let base = self.time_base;
        (pts != i64::MIN && base.den > 0)
            .then(|| pts as f64 * f64::from(base.num) / f64::from(base.den) - self.origin)
    }

    pub(crate) fn receive(&mut self) -> Result<bool, MediaError> {
        // FFmpeg guarantees that send and receive cannot both report EAGAIN.
        // The packet is unreferenced after each send and each skipped stream.
        unsafe {
            loop {
                let received = ((api().avcodec_receive_frame).unwrap())(self.codec, self.frame);
                match received {
                    0 => {
                        if let Some(target) = self.hls_seek_target.take()
                            && self.timestamp().is_some_and(|pts| pts + 0.025 < target)
                        {
                            self.hls_seek_target = Some(target);
                            continue;
                        }
                        return Ok(true);
                    }
                    ff::XRACK_FFMPEG_EOF => return Ok(false),
                    ff::XRACK_FFMPEG_AGAIN => {}
                    code => {
                        check(code, "decode video frame")?;
                    }
                }
                if self.draining {
                    return Err(MediaError(
                        "decoder requested packets after draining".into(),
                    ));
                }
                loop {
                    self.io.arm();
                    let code = ((api().av_read_frame).unwrap())(self.format, self.packet);
                    if code == ff::XRACK_FFMPEG_EOF {
                        check(
                            ((api().avcodec_send_packet).unwrap())(self.codec, ptr::null()),
                            "drain video decoder",
                        )?;
                        self.draining = true;
                        break;
                    }
                    check(code, "read video packet")?;
                    if self.hls {
                        self.packet_bytes += u64::from((*self.packet).size.max(0) as u32);
                    }
                    if (*self.packet).stream_index == self.stream {
                        let sent = ((api().avcodec_send_packet).unwrap())(self.codec, self.packet);
                        ((api().av_packet_unref).unwrap())(self.packet);
                        check(sent, "submit video packet")?;
                        break;
                    }
                    ((api().av_packet_unref).unwrap())(self.packet);
                }
            }
        }
    }

    fn convert(&mut self) -> Result<VideoFrame, MediaError> {
        // The successful receive owns a frame until the next receive or Drop.
        unsafe {
            let frame = &*self.frame;
            let (width, height) = (frame.width, frame.height);
            if !(1..=4096).contains(&width) || !(1..=4096).contains(&height) {
                return Err(MediaError(format!(
                    "video dimensions {width}×{height} are outside the supported range"
                )));
            }
            self.scale = ((api().sws_getCachedContext).unwrap())(
                self.scale,
                width,
                height,
                frame.format as ff::AVPixelFormat,
                width,
                height,
                ff::AVPixelFormat_AV_PIX_FMT_RGBA,
                ff::SwsFlags_SWS_BILINEAR as i32,
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null(),
            );
            if self.scale.is_null() {
                return Err(MediaError("create RGBA converter".into()));
            }
            let stride = width * 4;
            // swscale may write vector padding past the active final row.
            let byte_count = (stride * height) as usize;
            let mut rgba = vec![0; byte_count + 64];
            let sources = frame.data.map(|source| source.cast_const());
            let mut outputs = [
                rgba.as_mut_ptr(),
                ptr::null_mut(),
                ptr::null_mut(),
                ptr::null_mut(),
            ];
            let strides = [stride, 0, 0, 0];
            let rows = ((api().sws_scale).unwrap())(
                self.scale,
                sources.as_ptr(),
                frame.linesize.as_ptr(),
                0,
                height,
                outputs.as_mut_ptr(),
                strides.as_ptr(),
            );
            if rows != height {
                return Err(MediaError(format!(
                    "RGBA converter returned {rows} rows, expected {height}"
                )));
            }
            rgba.truncate(byte_count);
            Ok(VideoFrame {
                size: [width as u32, height as u32],
                rgba,
                timestamp_seconds: self.timestamp(),
            })
        }
    }
}

impl Drop for VideoDecoder {
    fn drop(&mut self) {
        unsafe {
            ((api().sws_freeContext).unwrap())(self.scale);
            ((api().av_packet_free).unwrap())(&mut self.packet);
            ((api().av_frame_free).unwrap())(&mut self.frame);
            ((api().avcodec_free_context).unwrap())(&mut self.codec);
            ((api().avformat_close_input).unwrap())(&mut self.format);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_local_file_is_reported() {
        let result = VideoDecoder::open(Path::new("/missing-xrack-test/video.mp4"));
        assert!(result.is_err());
    }

    #[test]
    fn reads_http_video_and_interrupts_blocked_open() {
        use std::{
            io::{Read, Write},
            net::TcpListener,
            sync::mpsc,
            thread,
            time::Duration,
        };
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let url = format!("http://{}/test.mp4", listener.local_addr().unwrap());
        let bytes = include_bytes!("../tests/sync.mp4");
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(2)))
                .unwrap();
            let mut request = [0; 4096];
            let _ = socket.read(&mut request);
            write!(
                socket,
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                bytes.len()
            )
            .unwrap();
            socket.write_all(bytes).unwrap();
        });
        let mut decoder = VideoDecoder::open(Path::new(&url)).unwrap();
        assert_eq!(decoder.next_frame().unwrap().unwrap().size, [160, 90]);
        drop(decoder);
        server.join().unwrap();

        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        let url = format!("http://{}/blocked.mp4", listener.local_addr().unwrap());
        let cancel = Arc::new(AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let (result, receiver) = mpsc::channel();
        let worker = thread::spawn(move || {
            let opened = VideoDecoder::open_video(Path::new(&url), worker_cancel);
            result.send(opened.is_err()).unwrap();
        });
        let (_socket, _) = listener.accept().unwrap();
        cancel.store(true, Ordering::Relaxed);
        assert!(receiver.recv_timeout(Duration::from_secs(2)).unwrap());
        worker.join().unwrap();
    }

    #[test]
    fn plays_live_hls_and_seeks_hls_vod_over_http() {
        use std::{
            io::{BufRead, BufReader, Write},
            net::TcpListener,
            thread,
            time::Duration,
        };
        let listener = TcpListener::bind(("127.0.0.1", 0)).unwrap();
        listener.set_nonblocking(true).unwrap();
        let origin = format!("http://{}", listener.local_addr().unwrap());
        let url = format!("{origin}/vod.m3u8");
        let done = Arc::new(AtomicBool::new(false));
        let shutdown = done.clone();
        let slide = Arc::new(AtomicBool::new(false));
        let sliding = slide.clone();
        let server = thread::spawn(move || {
            let live = include_str!("../tests/hls/vod.m3u8")
                .replace("#EXT-X-PLAYLIST-TYPE:VOD\n", "")
                .replace("#EXT-X-ENDLIST\n", "");
            let shifted = live
                .replace("#EXT-X-MEDIA-SEQUENCE:0", "#EXT-X-MEDIA-SEQUENCE:1")
                .replace("#EXTINF:1.000000,\nsegment0.ts\n", "");
            while !shutdown.load(Ordering::Relaxed) {
                let Ok((mut socket, _)) = listener.accept() else {
                    thread::sleep(Duration::from_millis(4));
                    continue;
                };
                socket
                    .set_read_timeout(Some(Duration::from_secs(1)))
                    .unwrap();
                let mut request = String::new();
                if BufReader::new(&socket).read_line(&mut request).is_err() {
                    continue;
                }
                let data: &[u8] = match request.split_whitespace().nth(1) {
                    Some("/vod.m3u8") => include_bytes!("../tests/hls/vod.m3u8"),
                    Some("/live.m3u8") if sliding.load(Ordering::Relaxed) => shifted.as_bytes(),
                    Some("/live.m3u8") => live.as_bytes(),
                    Some("/master.m3u8") => include_bytes!("../tests/hls/master.m3u8"),
                    Some("/high/vod.m3u8") => include_bytes!("../tests/hls/high/vod.m3u8"),
                    Some("/high/segment0.ts") => include_bytes!("../tests/hls/high/segment0.ts"),
                    Some("/high/segment1.ts") => include_bytes!("../tests/hls/high/segment1.ts"),
                    Some("/high/segment2.ts") => include_bytes!("../tests/hls/high/segment2.ts"),
                    Some("/segment0.ts") => include_bytes!("../tests/hls/segment0.ts"),
                    Some("/segment1.ts") => include_bytes!("../tests/hls/segment1.ts"),
                    Some("/segment2.ts") => include_bytes!("../tests/hls/segment2.ts"),
                    _ => {
                        let _ = socket.write_all(b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
                        continue;
                    }
                };
                let _ = write!(
                    socket,
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    data.len()
                )
                .and_then(|_| socket.write_all(data));
            }
        });
        let result = (|| {
            let mut decoder = VideoDecoder::open(Path::new(&url))?;
            if !decoder.seekable() || (decoder.duration() - 3.0).abs() >= 0.1 {
                return Err(MediaError(
                    "HLS VOD expected a seekable, 3-second timeline".into(),
                ));
            }
            if decoder
                .next_frame()?
                .is_none_or(|frame| frame.size != [160, 90])
            {
                return Err(MediaError(
                    "HLS VOD first frame could not be decoded".into(),
                ));
            }
            decoder.seek(1.2)?;
            let actual = decoder
                .next_frame()?
                .and_then(|frame| frame.timestamp_seconds);
            if actual.is_none_or(|pts| (pts - 1.2).abs() > 0.08) {
                return Err(MediaError(format!(
                    "HLS VOD seek to 1.2s returned {actual:?}"
                )));
            }
            let master = format!("{origin}/master.m3u8");
            for (variant, size, bitrate) in [(0, [160, 90], 90_000), (1, [320, 180], 360_000)] {
                let mut stream =
                    VideoDecoder::open_video_variant(Path::new(&master), variant, Arc::default())?
                        .ok_or_else(|| MediaError("missing HLS rendition".into()))?;
                let frame = stream
                    .next_frame()?
                    .ok_or_else(|| MediaError("missing HLS rendition frame".into()))?;
                if frame.size != size || stream.variants() != (variant, 2, bitrate) {
                    return Err(MediaError(format!(
                        "wrong HLS rendition: {:?} {:?}",
                        frame.size,
                        stream.variants()
                    )));
                }
                stream.seek(2.48)?;
                if stream.next_frame()?.is_none_or(|frame| {
                    frame.size != size
                        || frame
                            .timestamp_seconds
                            .is_none_or(|pts| (pts - 2.48).abs() > 0.08)
                }) {
                    return Err(MediaError(
                        "HLS rendition could not resume at the current position".into(),
                    ));
                }
            }
            let mut live = VideoDecoder::open(Path::new(&format!("{origin}/live.m3u8")))?;
            if !live.is_live()
                || live
                    .next_frame()?
                    .is_none_or(|frame| frame.size != [160, 90])
            {
                return Err(MediaError("expected a live HLS frame".into()));
            }
            let window = live
                .live_window(false)
                .ok_or_else(|| MediaError("missing live window".into()))?;
            if !live.seekable() || (window.end - window.start - 3.0).abs() > 0.08 {
                return Err(MediaError(format!(
                    "expected a 3s rewind window: {window:?}"
                )));
            }
            let target = window.start + 1.2;
            live.seek(target)?;
            let actual = live.next_frame()?.and_then(|frame| frame.timestamp_seconds);
            if actual.is_none_or(|pts| (pts - target).abs() > 0.12) {
                return Err(MediaError(format!(
                    "live seek to {target} returned {actual:?}"
                )));
            }
            slide.store(true, Ordering::Relaxed);
            let deadline = std::time::Instant::now() + Duration::from_secs(3);
            let shifted = loop {
                if let Some(shifted) = live.live_window(true)
                    && shifted.start > window.start + 0.8
                {
                    break shifted;
                }
                if std::time::Instant::now() >= deadline {
                    return Err(MediaError(
                        "live window did not evict its first segment".into(),
                    ));
                }
                thread::sleep(Duration::from_millis(20));
            };
            assert!((shifted.start - window.start - 1.0).abs() < 0.08);
            assert!((shifted.end - window.end).abs() < 0.08);
            live.seek(window.start)?;
            let clamped = live.next_frame()?.and_then(|frame| frame.timestamp_seconds);
            if clamped.is_none_or(|pts| (pts - shifted.start).abs() > 0.2) {
                return Err(MediaError(format!(
                    "expired live seek returned {clamped:?}"
                )));
            }
            Ok::<(), MediaError>(())
        })();
        done.store(true, Ordering::Relaxed);
        server.join().unwrap();
        result.unwrap();
    }

    #[test]
    fn selects_embedded_audio_and_text_subtitle_tracks() {
        let path = std::env::temp_dir().join(format!("xrack-tracks-{}.mp4", std::process::id()));
        std::fs::write(&path, include_bytes!("../tests/tracks.mp4")).unwrap();
        let audio = VideoDecoder::open_audio_track(&path, 1, 0, Arc::default())
            .unwrap()
            .unwrap();
        assert_eq!(audio.audio_tracks(), 2);
        assert_eq!(audio.subtitle_tracks(), 1);
        assert_eq!(audio.title(), "XRack tracks");
        let mut subtitle = VideoDecoder::open_subtitle_track(&path, 0, 0, Arc::default())
            .unwrap()
            .unwrap();
        let first = subtitle.next_cue().unwrap().unwrap();
        assert!((first.start - 0.5).abs() < 0.05);
        assert_eq!(first.text, "first caption");
        let second = subtitle.next_cue().unwrap().unwrap();
        assert!((second.start - 2.0).abs() < 0.05);
        assert_eq!(second.text, "second caption");
        drop((audio, subtitle));
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn decodes_multiple_frames_and_drains_video() {
        let path = std::env::temp_dir().join(format!("xrack-video-{}.y4m", std::process::id()));
        let mut data = b"YUV4MPEG2 W4 H4 F2:1 Ip A1:1 C420jpeg\n".to_vec();
        for luma in [16, 235] {
            data.extend(b"FRAME\n");
            data.extend([luma; 16]);
            data.extend([128; 8]);
        }
        std::fs::write(&path, data).unwrap();
        let mut decoder = VideoDecoder::open(&path).unwrap();
        let first = decoder.next_frame().unwrap().unwrap();
        let second = decoder.next_frame().unwrap().unwrap();
        assert_eq!(first.size, [4, 4]);
        assert_eq!(first.rgba.len(), 64);
        assert!(second.rgba[0] > first.rgba[0]);
        assert!(second.timestamp_seconds > first.timestamp_seconds);
        assert!(decoder.next_frame().unwrap().is_none());
        drop(decoder);
        std::fs::remove_file(path).unwrap();
    }
}
