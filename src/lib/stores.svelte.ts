import { invoke } from "@tauri-apps/api/core";

export interface DiscordMessage {
  author: string;
  content: string;
  encrypted: boolean;
}

export interface FetchResult {
  messages: DiscordMessage[];
  page_title: string;
  channel_url: string;
  dm_usernames: string[];
}

export interface ContactInfo {
  username: string;
  public_key_hex: string;
  fingerprint: string;
  handshake_status: string;
}

export interface HandshakeResult {
  status: string;
  message: string;
  contact_added: string | null;
  key_sent: boolean;
}

class AppState {
  messages: DiscordMessage[] = $state([]);
  statusText = $state("Disconnected");
  statusClass: string = $state("disconnected");
  connected = $state(false);
  connecting = $state(false);
  handshaking = $state(false);
  dmUsernames: string[] = $state([]);
  contacts: ContactInfo[] = $state([]);
  myPublicKey = $state("Loading…");
  keysOpen = $state(false);
  settingsOpen = $state(false);
}

export const app = new AppState();

export function setStatus(cls: string, text: string) {
  app.statusClass = cls;
  app.statusText = text;
}

export async function loadKeys() {
  try {
    app.myPublicKey = await invoke<string>("get_my_public_key");
    app.contacts = (await invoke<ContactInfo[]>("list_contacts")) || [];
  } catch (e) {
    console.error("loadKeys:", e);
  }
}
