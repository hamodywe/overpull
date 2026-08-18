// Proves the fixture crashes for the reason overpull claims.
// Exits 0 only on the exact ReferenceError; anything else is a failure.
try {
  await import('./entry.mjs');
  console.error('FAIL: expected a ReferenceError, module loaded cleanly');
  process.exit(1);
} catch (error) {
  const expected = "Cannot access 'SERVICE_NAME' before initialization";
  if (error instanceof ReferenceError && error.message.includes(expected)) {
    console.log(`OK: ${error.message}`);
    process.exit(0);
  }
  console.error(`FAIL: unexpected error: ${error}`);
  process.exit(1);
}
