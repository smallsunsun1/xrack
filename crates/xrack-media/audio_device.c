// Device output only. FFmpeg owns demuxing, decoding, and resampling.
#define MA_NO_DECODING
#define MA_NO_ENCODING
#define MA_NO_RESOURCE_MANAGER
#define MA_NO_NODE_GRAPH
#define MA_NO_ENGINE
#define MA_NO_GENERATION
#define MINIAUDIO_IMPLEMENTATION
#include "miniaudio.h"
#include <stdlib.h>

typedef void (*xr_audio_render)(void *, float *, unsigned int, unsigned int);

typedef struct XrackAudioDevice {
    ma_device device;
    xr_audio_render render;
    void *user;
} xr_audio_device;

static void output(ma_device *device, void *buffer, const void *input, ma_uint32 frames) {
    (void)input;
    xr_audio_device *audio = device->pUserData;
    ma_uint32 rate = device->playback.internalSampleRate;
    ma_uint32 delay = rate ? (ma_uint32)((ma_uint64)device->playback.internalPeriodSizeInFrames * 48000 / rate) : frames;
    audio->render(audio->user, buffer, frames, delay);
}

xr_audio_device *xr_audio_open(xr_audio_render render, void *user) {
    xr_audio_device *audio = calloc(1, sizeof(*audio));
    if (!audio)
        return NULL;
    audio->render = render;
    audio->user = user;
    ma_device_config config = ma_device_config_init(ma_device_type_playback);
    config.playback.format = ma_format_f32;
    config.playback.channels = 2;
    config.sampleRate = 48000;
    config.periodSizeInMilliseconds = 10;
    config.dataCallback = output;
    config.pUserData = audio;
    ma_backend null_backend = ma_backend_null;
    const char *test_device = getenv("XRACK_MEDIA_TEST_AUDIO");
    ma_bool32 use_null = test_device && test_device[0] == '1' && test_device[1] == '\0';
    ma_result result = use_null
        ? ma_device_init_ex(&null_backend, 1, NULL, &config, &audio->device)
        : ma_device_init(NULL, &config, &audio->device);
    if (result != MA_SUCCESS || (!use_null && audio->device.pContext->backend == ma_backend_null)) {
        if (result == MA_SUCCESS)
            ma_device_uninit(&audio->device);
        free(audio);
        return NULL;
    }
    if (ma_device_start(&audio->device) != MA_SUCCESS) {
        ma_device_uninit(&audio->device);
        free(audio);
        return NULL;
    }
    return audio;
}

void xr_audio_close(xr_audio_device *audio) {
    ma_device_uninit(&audio->device);
    free(audio);
}
