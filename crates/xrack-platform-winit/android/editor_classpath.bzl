"""Expose the Android and generated resource APIs used by the Java editor."""

load("@rules_android//providers:providers.bzl", "AndroidIdeInfo", "AndroidSdkInfo")

def _android_editor_classpath_impl(ctx):
    sdk = ctx.attr.sdk[AndroidSdkInfo]
    resources = ctx.attr.activity[AndroidIdeInfo].resource_jar
    sources = {
        "android.jar": sdk.android_jar,
        "activity-resources.jar": resources.class_jar,
    }
    jars = []
    for name, source in sources.items():
        jar = ctx.actions.declare_file(ctx.label.name + "/" + name)
        ctx.actions.symlink(output = jar, target_file = source)
        jars.append(jar)
    return [DefaultInfo(files = depset(jars))]

android_editor_classpath = rule(
    implementation = _android_editor_classpath_impl,
    attrs = {
        "activity": attr.label(mandatory = True, providers = [AndroidIdeInfo]),
        "sdk": attr.label(default = "@androidsdk//:sdk", providers = [AndroidSdkInfo]),
    },
)
