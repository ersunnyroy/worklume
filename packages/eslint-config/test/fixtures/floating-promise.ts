/** A promise that is neither awaited nor handled: errors would vanish silently. */
declare function saveTimesheet(): Promise<void>;

export function onSubmit(): void {
  saveTimesheet();
}
