// Proves both halves of the fixture behave as overpull claims:
// the invoked arrow throws, the identical uninvoked arrow does not.
const deferred = await import('./deferred-main.mjs').catch((error) => {
  console.error(`FAIL: the deferred half must load cleanly, got: ${error}`);
  process.exit(1);
});
if (typeof deferred.OUTPUT !== 'function') {
  console.error('FAIL: the deferred half did not export what it should');
  process.exit(1);
}

try {
  await import('./invoked-main.mjs');
  console.error('FAIL: expected a ReferenceError from the IIFE, module loaded cleanly');
  process.exit(1);
} catch (error) {
  const expected = "Cannot access 'NAME' before initialization";
  if (error instanceof ReferenceError && error.message.includes(expected)) {
    console.log(`OK: ${error.message}`);
    process.exit(0);
  }
  console.error(`FAIL: unexpected error: ${error}`);
  process.exit(1);
}
