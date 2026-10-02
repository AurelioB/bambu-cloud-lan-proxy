import type { Printer } from './api';
import { toast } from 'svelte-sonner';

export function needsAttention(printer: Printer) {
  return (
    !printer.online ||
    !printer.endpoint_enabled ||
    !printer.virtual_ip ||
    printer.endpoint_health.state !== 'healthy' ||
    !printer.secure_control
  );
}

export function printerState(printer: Printer) {
  if (!printer.online) return 'Offline';
  const labels: Record<string, string> = {
    idle: 'Ready',
    running: 'Printing',
    finished: 'Finished',
    finish: 'Finished',
    preparing: 'Preparing',
    prepare: 'Preparing',
    paused: 'Paused',
    pause: 'Paused',
    failed: 'Stopped',
    uncertain: 'Check printer',
    unknown: 'Status unknown',
  };
  return labels[printer.state.toLowerCase()] ?? 'Status unknown';
}

export function endpointLabel(printer: Printer) {
  if (!printer.endpoint_enabled) return 'Not enabled';
  if (!printer.virtual_ip) return 'Address needed';
  return {
    healthy: 'Connected',
    degraded: 'Needs attention',
    unavailable: 'Unavailable',
    disabled: 'Not enabled',
  }[printer.endpoint_health.state];
}

export function cameraLabel(printer: Printer) {
  if (
    printer.capabilities.source === 'unknown' ||
    printer.capabilities.camera === 'unknown'
  )
    return 'Support unknown';
  if (!printer.capabilities.local_camera_bridge)
    return 'Not supported by proxy';
  if (!printer.endpoint_enabled) return 'Not enabled';
  if (printer.endpoint_health.services?.camera === true)
    return 'Camera relay ready';
  if (printer.endpoint_health.services?.camera === false)
    return 'Camera unavailable';
  return 'Local setup needed';
}

export function toolheadLabel(printer: Printer) {
  const capabilities = printer.capabilities;
  if (
    (capabilities.model_known === false || capabilities.source === 'unknown') &&
    !capabilities.observed_fields?.includes('dual_nozzle')
  )
    return 'Capabilities unknown';
  return capabilities.dual_nozzle ? 'Dual nozzle' : 'Single nozzle';
}

export async function copyText(value: string) {
  try {
    await navigator.clipboard.writeText(value);
    toast.success('Copied to clipboard');
  } catch {
    toast.error('Clipboard unavailable. Select and copy the value instead.');
  }
}
