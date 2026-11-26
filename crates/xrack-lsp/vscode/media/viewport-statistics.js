// Shared by preview and attached-window webviews.
globalThis.formatViewportStatistics = function (report) {
    if (!report) return 'No 3D viewport work in this frame';
    const nameOf = view => view.id || ('SceneView3D ' + view.path.join('.')).trimEnd();
    const work = {
        inline: 'Inline', offscreen: 'Offscreen', retained: 'Reused texture', 'display-only': 'Display only',
    };
    const dimensions = size => size.join(' × ') + ' px';
    const lines = report.views.map(view => {
        const name = nameOf(view);
        const size = 'Output ' + dimensions(view.outputSize)
            + (view.rasterSize ? ' · Raster ' + dimensions(view.rasterSize) + ' · ' + view.samples + '× sampling' : '');
        const drawing = (view.cpuRecordMicros / 1000).toFixed(2) + ' ms CPU recording'
            + ' · ' + view.meshDraws + ' mesh color draws · ' + view.meshVertices + ' vertex references';
        const workCounts = [
            [view.spriteDraws, 'sprite draws'],
            [view.shadowPasses, 'shadow passes'],
            [view.reflectionFaces, 'reflection faces'],
            [view.capturePasses, 'capture passes'],
            [view.depthPrepassDraws, 'depth prepass draws'],
            [view.blendedResolves, 'weighted resolves'],
            [view.linkedResolves, 'linked resolves'],
            [view.meshUploadBytes, 'mesh upload bytes'],
            [view.linkedWorkingBytes, 'linked bytes'],
        ].filter(([count]) => count > 0).map(([count, label]) => count.toLocaleString() + ' ' + label);
        return [name + ' · ' + (work[view.work] ?? view.work), size, drawing, workCounts.join(' · ')]
            .filter(Boolean).join('\n');
    });
    if (lines.length) lines.unshift('CPU recording · frame ' + report.frame);
    else lines.push('No 3D viewport work in this frame');
    if (report.truncated) lines.push('Showing the first 64 viewport recordings');
    if (report.gpu?.views?.length) {
        const milliseconds = nanos => (nanos / 1_000_000).toFixed(3) + ' ms';
        lines.push('GPU pass times · frame ' + report.gpu.frame);
        for (const view of report.gpu.views) {
            const total = nameOf(view) + ' · ' + milliseconds(view.gpuNanos) + ' GPU pass sum';
            const passes = view.passes.map(pass => pass.name + ' · ' + pass.count + ' × · ' + milliseconds(pass.gpuNanos));
            lines.push([total, ...passes].join('\n'));
        }
        if (report.gpu.truncated) lines.push('GPU timings cover only part of the frame');
    } else if (!report.gpuTimingSupported) {
        lines.push('GPU timestamps unavailable on this device');
    }
    return lines.join('\n\n');
};
