import { describe, it, expect } from 'vitest';
import { deriveBackend } from '../stores/backend.svelte';

describe('deriveBackend', () => {
  it('maps a Metal device name to a friendly GPU label', () => {
    expect(deriveBackend('MTL0', true)).toEqual({ label: 'Metal GPU', state: 'gpu' });
    expect(deriveBackend('MTL', true)).toEqual({ label: 'Metal GPU', state: 'gpu' });
    // Case-insensitive substring match on "metal".
    expect(deriveBackend('Metal', true)).toEqual({ label: 'Metal GPU', state: 'gpu' });
  });

  it('maps CUDA and Vulkan device names', () => {
    expect(deriveBackend('CUDA0', true)).toEqual({ label: 'CUDA', state: 'gpu' });
    expect(deriveBackend('Vulkan0', true)).toEqual({ label: 'Vulkan', state: 'gpu' });
  });

  it('maps a CPU backend to the CPU label and state', () => {
    expect(deriveBackend('cpu', false)).toEqual({ label: 'CPU', state: 'cpu' });
    expect(deriveBackend('CPU', false)).toEqual({ label: 'CPU', state: 'cpu' });
  });

  it('treats an empty name as resolving regardless of the GPU flag', () => {
    expect(deriveBackend('', false)).toEqual({ label: 'Resolving…', state: 'resolving' });
    expect(deriveBackend('', true)).toEqual({ label: 'Resolving…', state: 'resolving' });
  });

  it('falls back to the raw name for an unknown non-empty GPU backend', () => {
    expect(deriveBackend('RPC0', true)).toEqual({ label: 'RPC0', state: 'gpu' });
  });

  it('classifies an unknown non-empty non-GPU backend as cpu', () => {
    // Not a recognised accelerator and not flagged GPU -> CPU state, raw label.
    expect(deriveBackend('SomethingElse', false)).toEqual({
      label: 'SomethingElse',
      state: 'cpu',
    });
  });
});
