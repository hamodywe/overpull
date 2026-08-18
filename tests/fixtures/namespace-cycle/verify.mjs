// Proves both halves of the fixture behave as overpull claims: reading a
// hoisted function off a not-yet-evaluated namespace is legal, reading a
// `const` off the same namespace is not.
const safe = await import('./safe-main.mjs').catch((error) => {
  console.error(`FAIL: the safe half must load cleanly, got: ${error}`);
  process.exit(1);
});
if (safe.OUTPUT !== 'greeting:a') {
  console.error(`FAIL: the safe half produced ${safe.OUTPUT}`);
  process.exit(1);
}

try {
  await import('./unsafe-main.mjs');
  console.error('FAIL: expected a ReferenceError, module loaded cleanly');
  process.exit(1);
} catch (error) {
  const expected = "Cannot access 'PREFIX' before initialization";
  if (error instanceof ReferenceError && error.message.includes(expected)) {
    console.log(`OK: ${error.message}`);
    process.exit(0);
  }
  console.error(`FAIL: unexpected error: ${error}`);
  process.exit(1);
}
