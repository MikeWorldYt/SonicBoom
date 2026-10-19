import type { SetupConstraints } from '../../services/contracts';
import { msg, type Message } from '../../i18n';
export interface SetupDraft { usage_mode: string; bind: string; port: number; auth_mode: string; admin_username: string; admin_password: string; inference_steps: number }
function address(raw: string): { loopback: boolean } | null {
  if (/^(?:\d+\.){3}\d+$/.test(raw)) {
    const parts = raw.split('.');
    return parts.every(part => Number(part) <= 255 && String(Number(part)) === part) ? { loopback: Number(parts[0]) === 127 } : null;
  }
  if (raw.includes(':') && !raw.includes('%') && !raw.includes('[') && !raw.includes(']')) {
    try { const host = new URL(`http://[${raw}]/`).hostname; return { loopback: host === '[::1]' }; } catch { return null; }
  }
  return null;
}
export function setupIssues(form: SetupDraft, constraints: SetupConstraints | null): Record<string, Message> {
  const issues: Record<string, Message> = {};
  if (!constraints) return { policy: msg('Setup constraints are unavailable. Refresh the page before continuing.') };
  if (!Number.isInteger(form.port) || form.port < 1 || form.port > 65535) issues.port = msg('Port must be between 1 and 65535.');
  if (form.usage_mode === 'advanced') {
    const ip = address(form.bind);
    if (!ip) issues.bind = msg('Enter a valid IPv4 or IPv6 bind address.');
    if (!constraints.auth_modes.includes(form.auth_mode)) issues.auth_mode = msg('Choose an allowed value.');
    else if (ip && !ip.loopback && (form.auth_mode === 'local' || (form.auth_mode === 'none' && !constraints.allow_insecure_remote))) issues.auth_mode = msg('Remote binding requires token authentication.');
  }
  if (!form.admin_username.trim()) issues.admin_username = msg('Enter an admin username.');
  if ([...form.admin_password].length < constraints.password_min_chars) issues.admin_password = msg('Admin password must contain at least {min} characters.', { min: constraints.password_min_chars });
  const asciiLower = (value: string) => value.replace(/[A-Z]/g, character => character.toLowerCase());
  if (constraints.rejected_passwords.some(value => asciiLower(value) === asciiLower(form.admin_password))) issues.admin_password = msg('Choose a password that is not a well-known default.');
  if (!Number.isInteger(form.inference_steps) || form.inference_steps < 1 || form.inference_steps > constraints.max_inference_steps) issues.inference_steps = msg('Inference steps must be between 1 and {max}.', { max: constraints.max_inference_steps });
  return issues;
}
