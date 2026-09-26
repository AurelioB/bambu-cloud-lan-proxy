export interface Printer {
  serial: string;
  name: string;
  model: string;
  virtual_ip: string;
  physical_ip: string;
  physical_access_configured: boolean;
  physical_access_code: string;
  endpoint_enabled: boolean;
  online: boolean;
  state: string;
  secure_control: boolean;
}
export interface Overview { healthy: boolean; signed_in: boolean; cloud_connected: boolean; region: string; printers: Printer[] }
export interface Settings {
  mode: "read_only" | "controls" | "full_control";
  allow_cancel: boolean;
  cloud_print_enabled: boolean;
  remote_reprint_enabled: boolean;
  advertise_controls: boolean;
  storage_allow_delete: boolean;
  storage_allow_upload: boolean;
}

export async function api<T = unknown>(path: string, accessCode: string, init: RequestInit = {}): Promise<T> {
  const response = await fetch(path, { ...init, headers: { "Content-Type": "application/json", "X-Cloud-Lan-Proxy-Key": accessCode, ...init.headers } });
  if (!response.ok) { throw new Error((await response.text()) || `Request failed (${response.status})`); }
  return response.status === 204 ? (undefined as T) : ((await response.json()) as T);
}
