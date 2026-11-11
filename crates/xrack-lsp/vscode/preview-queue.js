class PreviewQueue {
    constructor(render, delay = 200) {
        this.render = render;
        this.delay = delay;
        this.revision = 0;
        this.running = Promise.resolve();
        this.busy = false;
        this.disposed = false;
    }

    refresh(delay = this.delay) {
        if (this.disposed) return;
        ++this.revision;
        clearTimeout(this.timer);
        this.timer = setTimeout(() => {
            this.timer = undefined;
            this.start();
        }, delay);
    }

    start() {
        if (this.disposed || this.busy) return;
        const revision = this.revision;
        this.busy = true;
        this.running = this.render(() => !this.disposed && revision === this.revision)
            .finally(() => {
                this.busy = false;
                if (revision !== this.revision && !this.timer) this.start();
            });
    }

    dispose() {
        this.disposed = true;
        clearTimeout(this.timer);
        return this.running;
    }
}

module.exports = { PreviewQueue };
