// Builds the dev server and the neutral test package, then runs one
// api-dev-server per port with a fresh data folder (target/e2e/<port>).
import { spawn, spawnSync } from 'node:child_process';
import { rmSync, mkdirSync } from 'node:fs';
import { resolve, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

const root = resolve(dirname(fileURLToPath(import.meta.url)), '../../../../..');
const ports = process.argv.slice(2);
const cargo = process.env.CARGO ?? 'cargo';

function must(args) {
  const r = spawnSync(cargo, args, { cwd: root, stdio: 'inherit' });
  if (r.status !== 0) process.exit(r.status ?? 1);
}

must(['xtask', 'bundle', '--package', 'crates/core-config/tests/fixtures/club']);
must(['build', '-p', 'core-api', '--features', 'dev-server', '--bin', 'api-dev-server']);

const exe = resolve(root, 'target/debug/api-dev-server');
const bundle = resolve(root, 'target/package/club/bundle.bin');
const children = ports.map((port) => {
  const data = resolve(root, 'target/e2e', port);
  rmSync(data, { recursive: true, force: true });
  mkdirSync(data, { recursive: true });
  return spawn(exe, ['--data', data, '--bundle', bundle, '--port', port, '--network', 'local'], { stdio: 'inherit' });
});
const stop = () => children.forEach((c) => c.kill());
process.on('SIGINT', stop);
process.on('SIGTERM', stop);
process.on('exit', stop);
