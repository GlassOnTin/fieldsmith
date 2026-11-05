import { defineConfig, type Plugin } from 'vite';
import preact from '@preact/preset-vite';
import fs from 'node:fs';
import path from 'node:path';
import os from 'node:os';
import { spawnSync } from 'node:child_process';

/** Cross-origin isolation: shared wasm memory (the threaded build) needs both headers. A
 * host that omits them still works — the app falls back to the single-thread build. */
const isolation = {
  'Cross-Origin-Opener-Policy': 'same-origin',
  'Cross-Origin-Embedder-Policy': 'require-corp',
};

const repoRoot = path.resolve(__dirname, '..');
const workspaceFile = path.join(repoRoot, '.fieldsmith-workspace.json');

interface WorkspaceState {
  revision: number;
  updated_at_ms: number;
  actor: string;
  last_tool: string;
  doc_fsm: string | null;
  coupled_system_id: string | null;
  design_p: number[] | null;
  epistemic_theta: number[] | null;
  remote_compute_url?: string | null;
  activity_log: Array<{
    revision: number;
    timestamp_ms: number;
    actor: string;
    tool: string;
    summary: string;
  }>;
}

function readWorkspace(): WorkspaceState {
  try {
    if (fs.existsSync(workspaceFile)) {
      return JSON.parse(fs.readFileSync(workspaceFile, 'utf-8')) as WorkspaceState;
    }
  } catch {
    // fallback to default
  }
  return {
    revision: 0,
    updated_at_ms: Date.now(),
    actor: 'init',
    last_tool: 'none',
    doc_fsm: null,
    coupled_system_id: null,
    design_p: null,
    epistemic_theta: null,
    remote_compute_url: null,
    activity_log: [],
  };
}

function writeWorkspace(ws: WorkspaceState): void {
  fs.writeFileSync(workspaceFile, JSON.stringify(ws, null, 2), 'utf-8');
}

function resolveMcpBinary(): string {
  const rel = path.join(repoRoot, 'target', 'release', 'fieldsmith-mcp');
  if (fs.existsSync(rel)) return rel;
  return path.join(repoRoot, 'target', 'debug', 'fieldsmith-mcp');
}

function fieldsmithSharedWorkspacePlugin(): Plugin {
  return {
    name: 'fieldsmith-shared-workspace',
    configureServer(server) {
      server.middlewares.use((req, res, next) => {
        const url = req.url?.split('?')[0];
        if (url?.startsWith('/api/')) {
          res.setHeader('Access-Control-Allow-Origin', '*');
          res.setHeader('Access-Control-Allow-Methods', 'GET, POST, OPTIONS');
          res.setHeader('Access-Control-Allow-Headers', 'Content-Type, Authorization');
          if (req.method === 'OPTIONS') {
            res.statusCode = 204;
            res.end();
            return;
          }
        }
        if (url === '/api/health' && req.method === 'GET') {
          const ws = readWorkspace();
          res.setHeader('Content-Type', 'application/json');
          res.end(
            JSON.stringify({
              status: 'ok',
              server: 'fieldsmith-compute',
              version: '0.13.0',
              hostname: os.hostname(),
              os: os.platform(),
              arch: os.arch(),
              threads: os.cpus().length,
              listen_port: 5173,
              max_recommended_dofs: os.cpus().length >= 16 ? 500000 : 250000,
              workspace_revision: ws.revision,
              remote_compute_url: ws.remote_compute_url ?? null,
              capabilities: ['mcp', 'workspace', 'solve', 'coupled_systems', 'peer_discovery', 'compute_offload'],
            }),
          );
          return;
        }
        if ((url === '/api/peers/discover' || url === '/api/peers/connect') && (req.method === 'GET' || req.method === 'POST')) {
          let body = '';
          req.on('data', (chunk) => {
            body += chunk;
          });
          req.on('end', () => {
            try {
              const args = body.trim() ? JSON.parse(body) : {};
              const toolName = url === '/api/peers/discover' ? 'discover_compute_peers' : 'connect_compute_peer';
              const rpcReq = {
                jsonrpc: '2.0',
                id: 1,
                method: 'tools/call',
                params: {
                  name: toolName,
                  arguments: {
                    ports: [5173, 8080, 18789, 18791],
                    ...args,
                  },
                },
              };
              const bin = resolveMcpBinary();
              const proc = spawnSync(bin, [], {
                input: JSON.stringify(rpcReq) + '\n',
                encoding: 'utf-8',
                env: {
                  ...process.env,
                  FIELDSMITH_WORKSPACE: workspaceFile,
                },
                timeout: 15000,
                maxBuffer: 16 * 1024 * 1024,
              });
              const outLine = (proc.stdout || '').trim().split('\n').pop() || '{}';
              const parsed = JSON.parse(outLine);
              const text = parsed?.result?.content?.[0]?.text;
              let payload: unknown = parsed;
              if (typeof text === 'string') {
                try {
                  payload = JSON.parse(text);
                } catch {
                  payload = { ok: false, error: text };
                }
              }
              res.setHeader('Content-Type', 'application/json');
              res.end(JSON.stringify(payload));
            } catch (e) {
              res.statusCode = 400;
              res.setHeader('Content-Type', 'application/json');
              res.end(JSON.stringify({ error: String(e) }));
            }
          });
          return;
        }
        if (url === '/api/workspace' && req.method === 'GET') {
          res.setHeader('Content-Type', 'application/json');
          res.end(JSON.stringify(readWorkspace()));
          return;
        }
        if (url === '/api/workspace' && req.method === 'POST') {
          let body = '';
          req.on('data', (chunk) => {
            body += chunk;
          });
          req.on('end', () => {
            try {
              const incoming = JSON.parse(body || '{}');
              const ws = readWorkspace();
              ws.revision += 1;
              ws.updated_at_ms = Date.now();
              ws.actor = incoming.actor || 'web-ui';
              ws.last_tool = incoming.last_tool || 'ui-sync';
              if (typeof incoming.doc_fsm === 'string') ws.doc_fsm = incoming.doc_fsm;
              if (typeof incoming.coupled_system_id === 'string') {
                ws.coupled_system_id = incoming.coupled_system_id;
              }
              if (Array.isArray(incoming.design_p)) ws.design_p = incoming.design_p;
              if (Array.isArray(incoming.epistemic_theta)) {
                ws.epistemic_theta = incoming.epistemic_theta;
              }
              ws.activity_log = Array.isArray(ws.activity_log) ? ws.activity_log : [];
              ws.activity_log.push({
                revision: ws.revision,
                timestamp_ms: ws.updated_at_ms,
                actor: ws.actor,
                tool: ws.last_tool,
                summary: incoming.summary || `Updated from ${ws.actor}`,
              });
              if (ws.activity_log.length > 25) {
                ws.activity_log = ws.activity_log.slice(ws.activity_log.length - 25);
              }
              writeWorkspace(ws);
              res.setHeader('Content-Type', 'application/json');
              res.end(JSON.stringify(ws));
            } catch (e) {
              res.statusCode = 400;
              res.setHeader('Content-Type', 'application/json');
              res.end(JSON.stringify({ error: String(e) }));
            }
          });
          return;
        }
        if (url === '/api/mcp' && req.method === 'POST') {
          let body = '';
          req.on('data', (chunk) => {
            body += chunk;
          });
          req.on('end', () => {
            try {
              const parsed = JSON.parse(body || '{}');
              const rpcReq =
                parsed.jsonrpc === '2.0'
                  ? parsed
                  : {
                      jsonrpc: '2.0',
                      id: parsed.id ?? 1,
                      method: parsed.method ?? 'tools/call',
                      params: parsed.params ?? {
                        name: parsed.tool ?? parsed.name,
                        arguments: parsed.arguments ?? {},
                      },
                    };
              const bin = resolveMcpBinary();
              const proc = spawnSync(bin, [], {
                input: JSON.stringify(rpcReq) + '\n',
                encoding: 'utf-8',
                env: {
                  ...process.env,
                  FIELDSMITH_WORKSPACE: workspaceFile,
                },
                timeout: 30000,
                maxBuffer: 64 * 1024 * 1024,
              });
              const outLine = (proc.stdout || '').trim().split('\n').pop() || '{}';
              let rpcReply: unknown = {};
              try {
                rpcReply = JSON.parse(outLine);
              } catch {
                rpcReply = { raw: outLine, stderr: proc.stderr };
              }
              res.setHeader('Content-Type', 'application/json');
              res.end(
                JSON.stringify({
                  rpc: rpcReply,
                  workspace: readWorkspace(),
                }),
              );
            } catch (e) {
              res.statusCode = 400;
              res.setHeader('Content-Type', 'application/json');
              res.end(JSON.stringify({ error: String(e) }));
            }
          });
          return;
        }
        next();
      });
    },
  };
}

export default defineConfig({
  base: './',
  plugins: [preact(), fieldsmithSharedWorkspacePlugin()],
  build: {
    target: 'es2022',
    chunkSizeWarningLimit: 2000,
  },
  worker: {
    format: 'es',
  },
  server: {
    port: 5173,
    headers: isolation,
    // Example documents live in ../examples and are bundled with `?raw`.
    fs: { allow: ['..'] },
  },
  preview: {
    headers: isolation,
  },
});
