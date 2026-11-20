#include "native_api.h"
#include <libavdevice/avdevice.h>
#include <libavutil/avstring.h>
#include <libavutil/mem.h>

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

static const char *voice_default_device(const AVDeviceInfoList *list) {
    const char *first = NULL;
    for (int index = 0; index < list->nb_devices; ++index) {
        const AVDeviceInfo *device = list->devices[index];
        for (int type = 0; type < device->nb_media_types; ++type) {
            if (device->media_types[type] != AVMEDIA_TYPE_AUDIO)
                continue;
            if (index == list->default_device)
                return device->device_name;
            if (!first)
                first = device->device_name;
            break;
        }
    }
    return first;
}

// Optional, versioned separately from the playback table.
XRACK_MEDIA_EXPORT int xrack_voice_open_v1(AVFormatContext **context, const char *format_name,
                                         const char *source, AVDictionary **options) {
    if (!context || !*context || !format_name)
        return AVERROR(EINVAL);
    avdevice_register_all();
    const AVInputFormat *format = av_find_input_format(format_name);
    if (!format)
        return AVERROR(ENOSYS);

    char *default_source = NULL;
    if (!source) {
        // DirectShow requires a device name, unlike AVFoundation or ALSA.
        AVDeviceInfoList *devices = NULL;
        int result = avdevice_list_input_sources(format, NULL, NULL, &devices);
        const char *name = result >= 0 && devices ? voice_default_device(devices) : NULL;
        if (name)
            default_source = av_asprintf("audio=%s", name);
        else if (result >= 0)
            result = AVERROR(ENODEV);
        avdevice_free_list_devices(&devices);
        if (!default_source)
            return result < 0 ? result : AVERROR(ENOMEM);
        source = default_source;
    }

    (*context)->flags |= AVFMT_FLAG_NONBLOCK;
    int result = avformat_open_input(context, source, format, options);
    av_free(default_source);
    return result;
}
