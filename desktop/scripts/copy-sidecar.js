// Copy the built vaak-mcp sidecar binary to the binaries directory
// with the correct platform-specific name that Tauri expects.
import fs from 'fs';
import path from 'path';
import os from 'os';
import { fileURLToPath } from 'url';

const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

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

// Copy — handle EBUSY/EPERM on Windows (running sidecar processes lock the file)
try {
  fs.copyFileSync(src, dst);
} catch (err) {
  if (err.code === 'EBUSY' || err.code === 'EPERM') {
    // On Windows, a running .exe can sometimes be renamed but not overwritten.
    // Try multiple rename strategies:
    let renamed = false;
    for (let attempt = 0; attempt < 5; attempt++) {
      const suffix = attempt === 0 ? '.old' : `.old${attempt}`;
      const oldDst = dst + suffix;
      try { fs.unlinkSync(oldDst); } catch (_) { /* ignore */ }
      try {
        fs.renameSync(dst, oldDst);
        console.log(`Destination locked — renamed old binary to ${path.basename(oldDst)}`);
        renamed = true;
        break;
      } catch (renameErr) {
        console.log(`Rename attempt ${attempt} failed: ${renameErr.code}`);
      }
    }
    if (!renamed) {
      // If we can't even rename, try deleting the destination
      try {
        fs.unlinkSync(dst);
        console.log('Deleted locked destination file');
      } catch (delErr) {
        console.error(`Cannot rename or delete locked binary: ${delErr.code}`);
        console.error('Kill all vaak-mcp processes and try again.');
        process.exit(1);
      }
    }
    fs.copyFileSync(src, dst);
  } else {
    throw err;
  }
}
console.log(`Copied sidecar: ${dst}`);
