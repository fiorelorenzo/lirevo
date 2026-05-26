// Step → footer contract for the wizard router. Each step component
// receives a bindable `nextState` it can mutate so the wizard renders
// a coherent Next button in the footer. `onNextClick` may return
// `{ deferAdvance: true }` to signal that advancement is owned by the
// step (e.g. Cleanup, which waits for download:complete).
export interface NextClickResult {
  deferAdvance?: boolean;
}

export interface WizardStepState {
  canNext: boolean;
  nextLabel?: string;
  onNextClick?: () => void | NextClickResult | Promise<void | NextClickResult>;
}

export function defaultStepState(): WizardStepState {
  return { canNext: true };
}
