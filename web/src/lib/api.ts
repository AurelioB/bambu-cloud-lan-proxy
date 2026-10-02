export interface Printer {
  serial: string;
  name: string;
  model: string;
  virtual_ip: string;
  physical_ip: string;
  physical_access_configured: boolean;
  physical_access_code: string;
  endpoint_enabled: boolean;
  endpoint_health: {
    state: 'disabled' | 'healthy' | 'degraded' | 'unavailable';
    services: null | {
      process_running: boolean;
      mqtt: boolean;
      ftps: boolean;
      files: boolean;
      camera: boolean | null;
    };
  };
  online: boolean;
  state: string;
  secure_control: boolean;
  capabilities: {
    camera: 'rtsps' | 'image' | 'unknown';
    local_camera_bridge: boolean;
    chamber_temperature: boolean;
    dual_nozzle: boolean;
    source: 'model_profile' | 'runtime_telemetry' | 'unknown';
    model_known?: boolean;
    observed_fields?: string[];
    runtime?: null | {
      updated_at: number;
      firmware: string | null;
      ams_units: number | null;
      filament_slots: number | null;
      ams: Record<string, string[]>;
      sensors: string[];
      firmware_controls: Record<string, boolean>;
      dual_nozzle: boolean | null;
      chamber_temperature: boolean | null;
    };
  };
}
export interface Overview {
  healthy: boolean;
  signed_in: boolean;
  cloud_connected: boolean;
  region: string;
  printers: Printer[];
}
export interface Settings {
  mode: 'read_only' | 'controls' | 'full_control';
  allow_cancel: boolean;
  cloud_print_enabled: boolean;
  remote_reprint_enabled: boolean;
  advertise_controls: boolean;
  storage_allow_delete: boolean;
  storage_allow_upload: boolean;
}

export class ApiError extends Error {
  constructor(
    message: string,
    public status: number,
  ) {
    super(message);
  }
}

export interface AuthSession {
  authenticated: boolean;
  username: string | null;
  csrf_token: string | null;
  access_code: boolean;
  local: boolean;
  oidc: boolean;
}

let csrfToken = '';
export function setCsrfToken(value: string | null) {
  csrfToken = value ?? '';
}

export async function apiRequest(
  path: string,
  init: RequestInit = {},
): Promise<Response> {
  if (!path.startsWith('/') || path.startsWith('//'))
    throw new Error('Only same-origin API requests are allowed.');
  const headers = new Headers(init.headers);
  if (init.body) headers.set('Content-Type', 'application/json');
  if (csrfToken && !['GET', 'HEAD'].includes(init.method ?? 'GET'))
    headers.set('X-CSRF-Token', csrfToken);
  const response = await fetch(path, {
    ...init,
    credentials: 'same-origin',
    cache: 'no-store',
    headers,
  });
  if (!response.ok) {
    if (response.status === 401 && !path.startsWith('/api/auth/'))
      window.dispatchEvent(new Event('session-expired'));
    const text = await response.text();
    let message = text;
    try {
      message = JSON.parse(text).detail ?? text;
    } catch {
      /* Non-JSON upstream error. */
    }
    throw new ApiError(
      message || `Request failed (${response.status})`,
      response.status,
    );
  }
  return response;
}

export async function api<T = unknown>(
  path: string,
  init: RequestInit = {},
): Promise<T> {
  const response = await apiRequest(path, init);
  return response.status === 204
    ? (undefined as T)
    : ((await response.json()) as T);
}
