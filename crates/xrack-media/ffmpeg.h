#ifndef XRACK_MEDIA_FFMPEG_H
#define XRACK_MEDIA_FFMPEG_H

#include <errno.h>
#include <stddef.h>
#include <libavcodec/avcodec.h>
#include <libavformat/avformat.h>
#include <libavutil/error.h>
#include <libavutil/frame.h>
#include <libswresample/swresample.h>
#include <libswscale/swscale.h>

enum {
    XRACK_FFMPEG_AGAIN = AVERROR(EAGAIN),
    XRACK_FFMPEG_EOF = AVERROR_EOF,
    XRACK_FFMPEG_NOT_IMPLEMENTED = AVERROR(ENOSYS),
    XRACK_FFMPEG_STREAM_NOT_FOUND = AVERROR_STREAM_NOT_FOUND,
    XRACK_FFMPEG_SEEK_BACKWARD = AVSEEK_FLAG_BACKWARD,
    XRACK_FFMPEG_SEEK_ANY = AVSEEK_FLAG_ANY,
    // Bindgen 0.71 emits AVFormatContext as an opaque type with FFmpeg 9.
    XRACK_FFMPEG_STREAMS_OFFSET = offsetof(AVFormatContext, streams),
    XRACK_FFMPEG_START_OFFSET = offsetof(AVFormatContext, start_time),
    XRACK_FFMPEG_DURATION_OFFSET = offsetof(AVFormatContext, duration),
    XRACK_FFMPEG_INTERRUPT_OFFSET = offsetof(AVFormatContext, interrupt_callback),
    XRACK_FFMPEG_CONTEXT_FLAGS_OFFSET = offsetof(AVFormatContext, ctx_flags),
    XRACK_FFMPEG_UNSEEKABLE = AVFMTCTX_UNSEEKABLE,
    XRACK_FFMPEG_PROGRAMS_OFFSET = offsetof(AVFormatContext, programs),
    XRACK_FFMPEG_PROGRAM_COUNT_OFFSET = offsetof(AVFormatContext, nb_programs),
    XRACK_FFMPEG_FORMAT_OFFSET = offsetof(AVFormatContext, iformat),
    XRACK_FFMPEG_METADATA_OFFSET = offsetof(AVFormatContext, metadata),
    XRACK_FFMPEG_POINTER_OFFSET = offsetof(AVFormatContext, pb),
    XRACK_FFMPEG_STREAM_COUNT_OFFSET = offsetof(AVFormatContext, nb_streams),
    XRACK_FFMPEG_SEEKABLE_NORMAL = AVIO_SEEKABLE_NORMAL
};

#endif
