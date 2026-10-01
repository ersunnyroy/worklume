import axe from 'axe-core';

/**
 * Runs axe on the rendered document and returns readable violations. Colour contrast needs
 * real layout, so jsdom skips it; the Playwright smoke test checks it in a browser.
 */
export async function accessibilityViolations(
  root: Element | Document = document,
): Promise<string[]> {
  const results = await axe.run(root, { rules: { 'color-contrast': { enabled: false } } });
  return results.violations.map(
    (violation) => `${violation.id}: ${violation.help} (${String(violation.nodes.length)} nodes)`,
  );
}
