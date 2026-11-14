#include "native_api.h"

#if defined(_WIN32)
#define XRACK_MEDIA_EXPORT __declspec(dllexport)
#else
#define XRACK_MEDIA_EXPORT __attribute__((visibility("default")))
#endif

#if XRACK_ENABLE_HLS
int xrack_ffmpeg_hls_window(AVFormatContext *, int, int, int64_t *, int64_t *, int64_t *);
#endif

int xr_hls_window(AVFormatContext *format, int stream, int refresh,
                  int64_t *start, int64_t *end, int64_t *edge) {
#if XRACK_ENABLE_HLS
    return xrack_ffmpeg_hls_window(format, stream, refresh, start, end, edge);
#else
    (void)format;
    (void)stream;
    (void)refresh;
    (void)start;
    (void)end;
    (void)edge;
    return AVERROR(ENOSYS);
#endif
}

XRACK_MEDIA_EXPORT const XrackMediaApi *xrack_media_api_v3(void) {
    static const XrackMediaApi api = {
#define XRACK_MEDIA_ENTRY(name) .name = &name,
        XRACK_MEDIA_FUNCTIONS(XRACK_MEDIA_ENTRY)
#undef XRACK_MEDIA_ENTRY
    };
    return &api;
}
