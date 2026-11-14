#ifndef XRACK_MEDIA_NATIVE_API_H
#define XRACK_MEDIA_NATIVE_API_H

#include "ffmpeg.h"

typedef void (*xr_audio_render)(void *, float *, unsigned int, unsigned int);
typedef struct XrackAudioDevice XrackAudioDevice;
XrackAudioDevice *xr_audio_open(xr_audio_render render, void *user);
void xr_audio_close(XrackAudioDevice *audio);
int xr_hls_window(AVFormatContext *format, int stream, int refresh,
                  int64_t *start, int64_t *end, int64_t *edge);

// All FFmpeg entries are direct function pointers. The table and the Rust
// declarations are built from the same headers for each target.
#define XRACK_MEDIA_FUNCTIONS(X) \
    X(av_strerror) \
    X(av_channel_layout_default) \
    X(av_frame_alloc) \
    X(av_frame_free) \
    X(av_packet_alloc) \
    X(av_packet_free) \
    X(av_packet_unref) \
    X(avcodec_alloc_context3) \
    X(avcodec_free_context) \
    X(avcodec_parameters_to_context) \
    X(avcodec_open2) \
    X(avcodec_send_packet) \
    X(avcodec_receive_frame) \
    X(avcodec_flush_buffers) \
    X(avformat_open_input) \
    X(avformat_find_stream_info) \
    X(av_find_best_stream) \
    X(av_read_frame) \
    X(av_seek_frame) \
    X(avformat_close_input) \
    X(swr_init) \
    X(swr_alloc_set_opts2) \
    X(swr_free) \
    X(swr_convert) \
    X(swr_get_out_samples) \
    X(sws_freeContext) \
    X(sws_scale) \
    X(sws_getCachedContext) \
    X(xr_audio_open) \
    X(xr_audio_close) \
    X(avformat_alloc_context) \
    X(avcodec_find_decoder) \
    X(av_dict_set) \
    X(av_dict_get) \
    X(av_dict_free) \
    X(avcodec_decode_subtitle2) \
    X(avsubtitle_free) \
    X(xr_hls_window)

typedef struct XrackMediaApi {
#define XRACK_MEDIA_FIELD(name) __typeof__(&name) name;
    XRACK_MEDIA_FUNCTIONS(XRACK_MEDIA_FIELD)
#undef XRACK_MEDIA_FIELD
} XrackMediaApi;

const XrackMediaApi *xrack_media_api_v3(void);
#endif
