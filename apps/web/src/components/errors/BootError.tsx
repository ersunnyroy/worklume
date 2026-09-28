/**
 * Shown when the app cannot start (config.json missing or invalid, or a startup crash).
 * Translations may not be loaded at this point, so the text is plain English.
 */
export function BootError() {
  return (
    <main className="mx-auto max-w-xl p-8">
      <h1 className="text-2xl font-semibold">Worklume couldn't start</h1>
      <p className="mt-3 text-muted">
        Reload the page. If this keeps happening, contact support and mention the time it happened.
      </p>
    </main>
  );
}
