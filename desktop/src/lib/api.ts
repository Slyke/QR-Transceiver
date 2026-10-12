import { invoke, isTauri } from '@tauri-apps/api/core';
export const native = isTauri();
export const rpc = async <T>({ method, params = {} }: { method: string; params?: unknown }): Promise<T> => {
  // Callers own user-visible error handling.
  if (native) return await invoke<T>('rpc', { method, params });
  const response = await fetch('/dev-rpc', {
    method: 'POST',
    headers: { 'Content-Type': 'application/json' },
    body: JSON.stringify({ method, params })
  });
  const body = await response.json();
  if (!body.ok) throw new Error(body.error?.message ?? 'Local engine unavailable');
  return body.result as T;
};
export const bytes = ({ value }: { value: number }) => {
  if (value < 1024) return `${Math.round(value)} B`;
  const power = Math.min(Math.floor(Math.log(value) / Math.log(1024)), 4);
  return `${(value / 1024 ** power).toFixed(1)} ${['B', 'KiB', 'MiB', 'GiB', 'TiB'][power]}`;
};
export const duration = ({ seconds }: { seconds: number | null }) =>
  seconds === null ? 'Estimating…' : seconds < 60 ? `${Math.ceil(seconds)}s` : `${Math.floor(seconds / 60)}m ${Math.ceil(seconds % 60)}s`;
