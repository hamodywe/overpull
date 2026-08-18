// Proves both orders. The package entry loads cleanly; the test file, loaded
// on its own, throws — which is exactly why the verdict is
// `crash-if-loaded-first` and not `crash`.
const entry = await import('./src/index.mjs').catch((error) => {
  console.error(`FAIL: the package entry must load cleanly, got: ${error}`);
  process.exit(1);
});
if (typeof entry.build !== 'function') {
  console.error('FAIL: the package entry did not export what it should');
  process.exit(1);
}

// Fresh module graph, so the safe half is not already evaluated.
const { execFileSync } = await import('node:child_process');
const url = new URL('./tests/build.test.mjs', import.meta.url);
try {
  execFileSync(process.execPath, ['-e', `import(${JSON.stringify(url.href)})`], {
    stdio: 'pipe',
  });
  console.error('FAIL: expected the test file to throw when loaded first');
  process.exit(1);
} catch (error) {
  const message = String(error.stderr ?? error);
  if (message.includes("Cannot access 'DEFAULTS' before initialization")) {
    console.log("OK: loading the test file first throws on 'DEFAULTS'");
    process.exit(0);
  }
  console.error(`FAIL: unexpected error: ${message}`);
  process.exit(1);
}
