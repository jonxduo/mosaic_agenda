// Confirmations drawn by the app: window.confirm is not shown by the app's
// webview (it answers "no" without asking), so every question goes here.
export const question = $state<{ current: { text: string; ok: string; danger: boolean; resolve: (yes: boolean) => void } | null }>({
  current: null
});

export function ask(text: string, ok: string, danger = false): Promise<boolean> {
  question.current?.resolve(false);
  return new Promise((resolve) => {
    question.current = { text, ok, danger, resolve };
  });
}

export function answer(yes: boolean) {
  const q = question.current;
  question.current = null;
  q?.resolve(yes);
}
