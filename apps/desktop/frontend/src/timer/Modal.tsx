import { useEffect, useRef, type ReactNode } from 'react';

interface ModalProps {
  readonly labelledBy: string;
  readonly describedBy: string;
  readonly children: ReactNode;
}

/**
 * A question the member must answer: a native modal dialog, so the webview traps focus in it
 * and makes the rest of the window inert. Escape doesn't dismiss it, because only an answer
 * can. Focus starts on the element marked `data-initial-focus`.
 */
export function Modal({ labelledBy, describedBy, children }: ModalProps) {
  const ref = useRef<HTMLDialogElement>(null);

  useEffect(() => {
    const dialog = ref.current;
    if (dialog === null) {
      return undefined;
    }
    if (!dialog.open) {
      dialog.showModal();
    }
    dialog.querySelector<HTMLElement>('[data-initial-focus]')?.focus();
    return () => {
      if (dialog.open) {
        dialog.close();
      }
    };
  }, []);

  return (
    <dialog
      ref={ref}
      className="question"
      role="alertdialog"
      aria-labelledby={labelledBy}
      aria-describedby={describedBy}
      onCancel={(event) => {
        event.preventDefault();
      }}
    >
      {children}
    </dialog>
  );
}
