const net = require('node:net');

function readReply(socketPath, query, signal) {
    return new Promise((resolve, reject) => {
        const socket = net.createConnection({ path: socketPath, signal });
        let text = '';
        let bytes = 0;
        if (query) socket.on('connect', () => socket.end(JSON.stringify(query) + '\n'));
        socket.setEncoding('utf8');
        socket.setTimeout(2500, () => socket.destroy(new Error('Timed out waiting for the application')));
        socket.on('data', chunk => {
            text += chunk;
            bytes += Buffer.byteLength(chunk);
            if (bytes > 4 * 1024 * 1024) socket.destroy(new Error('Invalid XRack snapshot size'));
        });
        socket.on('error', reject);
        socket.on('end', () => {
            try {
                const parsed = JSON.parse(text);
                if (parsed.error) throw new Error(parsed.error);
                if (!query && (parsed.version !== 1 || !Array.isArray(parsed.properties))) {
                    throw new Error('Incompatible XRack inspection protocol');
                }
                resolve(parsed);
            } catch (error) {
                reject(error);
            }
        });
    });
}

module.exports = { readReply };
