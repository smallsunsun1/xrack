const { spawn } = require('node:child_process');
const files = require('node:fs/promises');

class PreviewProcess {
    constructor(program, component, cwd) {
        this.program = program;
        this.child = spawn(program, [component], { cwd, windowsHide: true });
        this.stderr = '';
        this.remaining = Buffer.alloc(0);
        this.stdout = this.child.stdout[Symbol.asyncIterator]();
        this.pending = Promise.resolve();
        this.exited = new Promise(resolve => this.child.once('close', resolve));
        this.child.on('error', error => { this.error = error; });
        this.child.stdin.on('error', () => {});
        this.child.stderr.setEncoding('utf8');
        this.child.stderr.on('data', text => {
            this.stderr = (this.stderr + text).slice(-65536);
        });
    }

    get alive() {
        return !this.error && !this.stopped && this.child.exitCode === null && this.child.signalCode === null;
    }

    async read(length) {
        const data = Buffer.allocUnsafe(length);
        let offset = 0;
        while (offset < length) {
            if (this.remaining.length === 0) {
                const { value, done } = await this.stdout.next();
                if (done) throw new Error('Preview renderer stopped');
                this.remaining = value;
            }
            const bytes = this.remaining.copy(data, offset);
            offset += bytes;
            this.remaining = this.remaining.subarray(bytes);
        }
        return data;
    }

    frame(command) {
        const request = this.pending.then(async () => {
            let timedOut = false;
            const timeout = setTimeout(() => {
                timedOut = true;
                this.child.kill();
            }, 30_000);
            try {
                if (command) {
                    await new Promise((resolve, reject) => this.child.stdin.write(command + '\n',
                        error => error ? reject(error) : resolve()));
                }
                const length = (await this.read(4)).readUInt32BE();
                if (!length || length > 32 * 1024 * 1024) throw new Error('Invalid preview frame length');
                return (await this.read(length)).toString('base64');
            } catch (error) {
                void this.stop();
                if (timedOut) throw new Error('Preview renderer timed out');
                throw new Error(this.stderr.trim() || this.error?.message || error.message);
            } finally {
                clearTimeout(timeout);
            }
        });
        this.pending = request.catch(() => {});
        return request;
    }

    stop() {
        if (!this.stopped) {
            this.child.kill();
            this.stopped = this.exited.then(() => files.rm(this.program, { force: true })).catch(() => {});
        }
        return this.stopped;
    }
}

module.exports = { PreviewProcess };
