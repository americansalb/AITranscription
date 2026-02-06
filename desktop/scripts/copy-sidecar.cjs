// Copy the built vaak-mcp sidecar binary to the binaries directory
// with the correct platform-specific name that Tauri expects.
const fs = require('fs');
const path = require('path');
const os = require('os');

const tauriDir = path.join(__dirname, '..', 'src-tauri');
const binariesDir = path.join(tauriDir, 'binaries');
const releaseDir = path.join(tauriDir, 'target', 'release');

// Determine target triple
const platform = os.platform();
const arch = os.arch();

let triple, ext;
if (platform === 'darwin') {
  triple = arch === 'arm64' ? 'aarch64-apple-darwin' : 'x86_64-apple-darwin';
  ext = '';
} else if (platform === 'linux') {
  triple = 'x86_64-unknown-linux-gnu';
  ext = '';
} else if (platform === 'win32') {
  triple = 'x86_64-pc-windows-msvc';
  ext = '.exe';
} else {
  console.error('Unknown platform:', platform);
  process.exit(1);
}

const srcName = `vaak-mcp${ext}`;
const dstName = `vaak-mcp-${triple}${ext}`;
const src = path.join(releaseDir, srcName);
const dst = path.join(binariesDir, dstName);

// Create binaries dir if needed
fs.mkdirSync(binariesDir, { recursive: true });

// Copy
fs.copyFileSync(src, dst);
console.log(`Copied sidecar: ${dst}`);
