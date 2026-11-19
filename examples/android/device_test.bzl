"""APK arguments for Android device tests that run on the host."""

load("@rules_python//python:defs.bzl", "py_test")

def android_device_test(name, apk, extra_apks = {}, tags = [], **kwargs):
    apk_tags = ["xrack-device-apk:" + apk] + [
        "xrack-device-extra:" + argument + "=" + artifact
        for argument, artifact in extra_apks.items()
    ]
    py_test(
        name = name,
        tags = tags + apk_tags,
        **kwargs
    )
