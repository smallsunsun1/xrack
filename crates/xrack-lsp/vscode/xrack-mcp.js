#!/usr/bin/env node
const fs = require('node:fs/promises');
const path = require('node:path');
const readline = require('node:readline');
const { setTimeout: delay } = require('node:timers/promises');
const { readReply } = require('./live-client');

const protocols = ['2025-11-25', '2025-06-18', '2025-03-26'];
const string = { type: 'string', minLength: 1, maxLength: 1024 };
const integer = (minimum, maximum) => ({ type: 'integer', minimum, maximum });
const schema = (properties, required = Object.keys(properties)) =>
    ({ type: 'object', properties, required, additionalProperties: false });
const target = { session: string, nodeKey: string };

const controlTools = new Set(['click', 'fill']);
const generation = integer(0, Number.MAX_SAFE_INTEGER);
const tools = {
    click: ['Click a unique named button in the live app at the specified source generation.',
        schema({ session: string, id: string, generation })],
    fill: ['Replace the focused value of a named text input in an opted-in live app.',
        schema({ session: string, id: string, generation, text: { type: 'string', minLength: 0, maxLength: 2048 } })],
    get_recording: ['Export the opt-in, named native UI actions as a versioned replay recording.',
        schema({ session: string })],
    get_update_profile: ['Read the compiler update plan, CPU samples, and 3D viewport statistics from native application updates.',
        schema({ session: string, element: string }, ['session'])],
    wait_for_property: ['Wait for a typed property value or model row count to equal the requested value.',
        schema({ session: string, nodeKey: string, property: string,
            field: { enum: ['value', 'count'] }, equals: { type: ['string', 'boolean', 'number'] },
            timeoutMs: integer(1, 10000) }, ['session', 'property', 'equals'])],
    list_sessions: ['Find live XRack development sessions in this project.', schema({})],
    get_ui_tree: ['Read the running component tree with node keys and source locations.',
        schema({ session: string })],
    get_node: ['Read typed properties and source locations for a component or element.',
        schema(target)],
    read_model: ['Read a page from a public string or record model. Compare modelRevision between pages.',
        schema({ ...target, property: string, offset: integer(0, Number.MAX_SAFE_INTEGER),
            limit: integer(1, 24), generation: integer(0, Number.MAX_SAFE_INTEGER) },
        ['session', 'nodeKey', 'property'])],
    get_diagnostics: ['Read hot-reload or rendering errors and versions of the applied source.',
        schema({ session: string })],
    get_calls: ['Read recent completed XR handler and callback frames from the live application.',
        schema({ session: string, limit: integer(1, 48) }, ['session'])],
    get_call_stack: ['Read a captured XR call and its retained caller frames, oldest caller first.',
        schema({ session: string, frame: integer(1, Number.MAX_SAFE_INTEGER) })],
};

async function discover(project) {
    const directory = path.join(project, '.xrack-live');
    const entries = await fs.readdir(directory, { withFileTypes: true }).catch(error => {
        if (error.code === 'ENOENT') return [];
        throw error;
    });
    const sessions = [];
    for (const entry of entries) {
        const match = /^(\d+)-[0-9a-f]+\.json$/.exec(entry.name);
        if (!match || !entry.isFile()) continue;
        try {
            const file = path.join(directory, entry.name);
            if ((await fs.stat(file)).size > 8192) continue;
            const app = JSON.parse(await fs.readFile(file, 'utf8'));
            const id = path.basename(entry.name, '.json');
            if (app.version !== 1 || app.pid !== Number(match[1]) || app.pid < 1
                || typeof app.component !== 'string' || typeof app.socket !== 'string'
                || !path.isAbsolute(app.socket) || path.basename(app.socket) !== 's') continue;
            const socketDir = path.dirname(app.socket);
            if (path.basename(socketDir) !== 'xra-' + id) continue;
            const owner = await fs.lstat(socketDir);
            if (!owner.isDirectory() || owner.uid !== process.getuid() || (owner.mode & 0o077)) continue;
            process.kill(app.pid, 0);
            if (!(await fs.lstat(app.socket)).isSocket()) continue;
            const querySocket = path.join(socketDir, 'q');
            const modelQueries = app.querySocket === querySocket
                && await fs.lstat(querySocket).then(stat => stat.isSocket(), () => false);
            sessions.push({ id, pid: app.pid, component: app.component, source: app.source,
                socket: app.socket, querySocket: modelQueries ? querySocket : undefined });
        } catch {
            // A session can disappear while discovery is in progress.
        }
    }
    return sessions;
}

function validate(args, schema) {
    if (!args || typeof args !== 'object' || Array.isArray(args)) throw new Error('Expected tool arguments');
    for (const key of schema.required) {
        if (!Object.hasOwn(args, key)) throw new Error(`Missing argument: ${key}`);
    }
    for (const [key, value] of Object.entries(args)) {
        const rule = Object.hasOwn(schema.properties, key) && schema.properties[key];
        const valid = rule && (rule.enum ? rule.enum.includes(value)
            : Array.isArray(rule.type) ? rule.type.includes(typeof value)
            : rule.type === 'string'
                ? typeof value === 'string' && value.length >= rule.minLength && value.length <= rule.maxLength
                : Number.isSafeInteger(value) && value >= rule.minimum && value <= rule.maximum);
        if (!valid) throw new Error(`Invalid argument: ${key}`);
    }
}

function callStack(frames, id) {
    const byId = new Map(frames.map(frame => [frame.id, frame]));
    const stack = [];
    for (let frame = byId.get(id); frame; frame = byId.get(frame.parentId)) {
        stack.unshift(frame);
        // Real parent IDs always precede their children.
        if (frame.parentId >= frame.id) break;
    }
    return stack;
}

function sessionState(session, snapshot) {
    return { session: session.id, generation: snapshot.trace?.generation, sourceVersions: snapshot.sourceVersions };
}

async function waitForProperty(session, args) {
    const deadline = performance.now() + (args.timeoutMs ?? 2000);
    const field = args.field ?? 'value';
    const nodeKey = args.nodeKey ?? 'root';
    let result = { session: session.id, matched: false };
    for (;;) {
        const remaining = deadline - performance.now();
        if (remaining <= 0) return result;
        let snapshot;
        try {
            snapshot = await readReply(session.socket, undefined, AbortSignal.timeout(Math.ceil(remaining)));
        } catch (error) {
            if (error.code === 'ABORT_ERR') return result;
            throw error;
        }
        const node = snapshot.tree?.nodes.find(node => node.key === nodeKey);
        const rows = nodeKey === 'root' ? snapshot.properties : [
            ...(node?.componentProperties ?? []), ...(node?.properties ?? []),
        ];
        const property = rows.find(property => property.name === args.property);
        result = { ...sessionState(session, snapshot), property, matched: property?.[field] === args.equals };
        if (result.matched) return result;
        const pause = Math.min(80, deadline - performance.now());
        if (pause > 0) await delay(pause);
    }
}

async function call(project, name, args) {
    if (!Object.hasOwn(tools, name)) throw new Error('Unknown XRack tool');
    validate(args, tools[name][1]);
    const sessions = await discover(project);
    if (name === 'list_sessions') {
        return { sessions: sessions.map(({ id, pid, component, source, querySocket }) =>
            ({ id, pid, component, source, modelQueries: Boolean(querySocket) })) };
    }
    const session = sessions.find(app => app.id === args.session);
    if (!session) throw new Error('XRack session has ended or is outside this project');
    if (controlTools.has(name)) {
        if (!session.querySocket) throw new Error('This app was built without live inspection actions');
        return { session: session.id, ...await readReply(session.querySocket, {
            generation: args.generation,
            input: { type: name, id: args.id, text: args.text },
        }) };
    }
    if (name === 'read_model') {
        if (!session.querySocket) throw new Error('This app was built without paged model inspection');
        return { session: session.id, ...await readReply(session.querySocket, {
            nodeKey: args.nodeKey, property: args.property, offset: args.offset ?? 0,
            limit: args.limit ?? 12, generation: args.generation,
        }) };
    }
    if (name === 'wait_for_property') return waitForProperty(session, args);
    const snapshot = await readReply(session.socket);
    const result = sessionState(session, snapshot);
    switch (name) {
        case 'get_recording':
            if (!snapshot.recording) throw new Error('Launch the live app with XRACK_RECORD_UI=1');
            return snapshot.recording;
        case 'get_update_profile': {
            if (!snapshot.updatePlan) throw new Error('Launch the live app with XRACK_PROFILE_UI=1');
            const plan = snapshot.updatePlan;
            const entries = args.element ? plan.entries.filter(entry => entry.element === args.element) : plan.entries;
            return {
                ...result,
                plan: { ...plan, entries },
                updates: snapshot.updates ?? [],
                renderSamples: snapshot.renderSamples ?? [],
                presentedFrames: snapshot.presentedFrames ?? [],
                viewportStatistics: snapshot.viewportStatistics ?? null,
            };
        }
        case 'get_ui_tree':
            return { ...result, truncated: snapshot.tree?.truncated ?? false,
                nodes: (snapshot.tree?.nodes ?? []).map(({ key, parent, kind, name, source, componentSource }) =>
                    ({ key, parent, kind, name, source, componentSource })) };
        case 'get_node': {
            const node = snapshot.tree?.nodes.find(node => node.key === args.nodeKey);
            if (!node && args.nodeKey !== 'root') throw new Error('Node is absent from the current inspected tree');
            return { ...result, node, componentProperties: args.nodeKey === 'root'
                ? snapshot.properties : node.componentProperties };
        }
        case 'get_diagnostics':
            return { ...result, reloadDiagnostic: snapshot.reloadDiagnostic, treeError: snapshot.treeError };
        case 'get_calls':
            return { ...result, captureValues: snapshot.trace?.captureValues ?? false,
                frames: (snapshot.trace?.frames ?? []).slice(-(args.limit ?? 16)).reverse() };
        case 'get_call_stack': {
            const stack = callStack(snapshot.trace?.frames ?? [], args.frame);
            if (!stack.length) throw new Error('Call frame is absent from the retained history');
            return { ...result, truncated: stack[0].parentId != null, stack };
        }
    }
}

async function serve(project) {
    const lines = readline.createInterface({ input: process.stdin, crlfDelay: Infinity, terminal: false });
    let initialized = false;
    const send = message => process.stdout.write(JSON.stringify({ jsonrpc: '2.0', ...message }) + '\n');
    for await (const line of lines) {
        let request;
        try {
            request = line.length <= 65536 ? JSON.parse(line) : null;
        } catch {
            send({ id: null, error: { code: -32700, message: 'Invalid JSON' } });
            continue;
        }
        if (!request || request.jsonrpc !== '2.0' || typeof request.method !== 'string') {
            send({ id: null, error: { code: -32600, message: 'Invalid JSON-RPC request' } });
            continue;
        }
        if (!Object.hasOwn(request, 'id')) continue;
        const { id, method, params } = request;
        if (method === 'initialize') {
            initialized = true;
            send({ id, result: {
                protocolVersion: protocols.includes(params?.protocolVersion) ? params.protocolVersion : protocols[0],
                capabilities: { tools: {} },
                serverInfo: { name: 'xrack-devtools', version: '0.1.0' },
            } });
        } else if (method === 'ping') {
            send({ id, result: {} });
        } else if (!initialized) {
            send({ id, error: { code: -32002, message: 'Initialize the MCP session first' } });
        } else if (method === 'tools/list') {
            send({ id, result: { tools: Object.entries(tools).map(([name, [description, inputSchema]]) =>
                ({ name, description, inputSchema, annotations: { readOnlyHint: !controlTools.has(name), openWorldHint: controlTools.has(name) } })) } });
        } else if (method === 'tools/call') {
            try {
                const value = await call(project, params?.name, params?.arguments ?? {});
                send({ id, result: { structuredContent: value,
                    content: [{ type: 'text', text: JSON.stringify(value) }] } });
            } catch (error) {
                send({ id, result: { isError: true, content: [{ type: 'text', text: error.message }] } });
            }
        } else {
            send({ id, error: { code: -32601, message: 'Method not found' } });
        }
    }
}

if (require.main === module) {
    const [flag, project, ...extra] = process.argv.slice(2);
    if (flag !== '--project' || !project || extra.length || process.platform === 'win32') {
        console.error('Usage on macOS or Linux: node xrack-mcp.js --project <workspace-directory>');
        process.exitCode = 2;
    } else {
        serve(path.resolve(project)).catch(error => {
            console.error(error.message);
            process.exitCode = 1;
        });
    }
}
