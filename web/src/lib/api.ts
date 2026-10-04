export type Category = 'important' | 'feed' | 'junk';

export interface User {
	id: number;
	email: string;
	is_admin: boolean;
}

export interface Account {
	id: number;
	label: string;
	address: string;
	display_name: string;
	imap_host: string;
	imap_port: number;
	imap_security: string;
	imap_username: string;
	smtp_host: string;
	smtp_port: number;
	smtp_security: string;
	smtp_username: string;
	inbox_folder: string;
	junk_folder: string;
	sent_folder: string;
	trash_folder: string;
	archive_folder: string;
	/** Server folder for mail from Nice to know senders; empty keeps it in the inbox. */
	feed_folder: string;
	/** Server folder delayed conversations wait in; empty leaves them where they are. */
	delayed_folder: string;
	append_sent: boolean;
	last_error: string | null;
	last_sync_at: number | null;
}

export interface Addr {
	name: string;
	address: string;
}

export interface ScreenerEntry {
	id: number;
	address: string;
	display_name: string;
	count: number;
	message_id: number;
	thread_id: number;
	subject: string;
	snippet: string;
	date: number;
}

export interface Sender {
	id: number;
	address: string;
	display_name: string;
	category: Category | null;
	decided_at: number | null;
	count: number;
	/** A picture of your own choosing is set for the sender. */
	has_picture: boolean;
}

export interface ThreadSummary {
	id: number;
	subject: string;
	count: number;
	unread: number;
	date: number;
	snippet: string;
	from_name: string;
	from_addr: string;
	is_outgoing: boolean;
	account_id: number;
	has_attachments: boolean;
	sender_name: string | null;
	sender_address: string | null;
	/** When a delayed conversation returns to the inbox. */
	snoozed_until: number | null;
	/** Your own note on the conversation; empty when there is none. */
	note: string;
}

export interface Attachment {
	idx: number;
	filename: string;
	mime: string;
	size: number;
	kind: 'image' | 'pdf' | 'office' | 'other';
}

export interface Message {
	id: number;
	thread_id: number;
	account_id: number;
	sender_id: number | null;
	sender_category: Category | null;
	show_images: boolean;
	from: Addr;
	to: Addr[];
	cc: Addr[];
	subject: string;
	date: number;
	seen: boolean;
	is_outgoing: boolean;
	body_text: string;
	body_html: string;
	attachments: Attachment[];
}

export interface Thread {
	id: number;
	/** Some received mail of it is still in the inbox, so it can be archived. */
	can_archive: boolean;
	/** Some mail of it is not in the Trash yet. */
	can_trash: boolean;
	/** Some mail of it is in the Trash and can be moved back. */
	can_restore: boolean;
	/** It is in the Important list (a mail of it is flagged). */
	important: boolean;
	/** Delayed until then, or null. */
	snoozed_until: number | null;
	/** Your own note on the conversation; empty when there is none. */
	note: string;
	subject: string;
	sender: { id: number; address: string; display_name: string; category: Category | null; has_picture: boolean } | null;
	messages: Message[];
}

export interface Draft {
	id: number;
	account_id: number | null;
	kind: 'new' | 'reply' | 'reply_all' | 'forward';
	source_message: number | null;
	to_addrs: string;
	cc_addrs: string;
	bcc_addrs: string;
	subject: string;
	body_html: string;
	forward_attachments: boolean;
	include_quote: boolean;
	updated_at: number;
	/** Why the last attempt to send it failed, if one did. */
	last_error: string | null;
}

export interface DraftAttachment {
	id: number;
	filename: string;
	mime: string;
	size: number;
}

export interface DraftDetail {
	draft: Draft;
	attachments: DraftAttachment[];
	source: { id: number; from: Addr; subject: string; date: number; attachments: number } | null;
}

export interface SearchHit {
	id: number;
	thread_id: number;
	account_id: number;
	subject: string;
	from_name: string;
	from_addr: string;
	date: number;
	seen: boolean;
	excerpt: string;
	/** Your note on the conversation the mail belongs to. */
	note: string;
	/** The list the mail is found in. */
	place: 'home' | 'flagged' | 'delayed' | 'feed' | 'screener' | 'archive' | 'sent' | 'junk' | 'trash';
}

export interface SavedSearch {
	id: number;
	name: string;
	query: string;
	/** Unread received mails the search finds right now. */
	unread: number;
}

export interface Counts {
	screener: number;
	/** Unread conversations in Home (named after its sender category). */
	unread_important: number;
	/** Unread conversations in the Important list. */
	unread_flagged: number;
	/** Unread conversations in Nice to know, Junk, Delayed, Archive and Trash. */
	unread_feed: number;
	unread_junk: number;
	unread_delayed: number;
	unread_archive: number;
	unread_trash: number;
	delayed: number;
	drafts: number;
}

/** One unseen mail of Home with what the local model says about it. */
export interface BriefingEntry {
	message_id: number;
	thread_id: number;
	from_name: string;
	from_addr: string;
	subject: string;
	date: number;
	summary: string;
	attachments: { filename: string; text: string }[];
	/** The summary is still being written. */
	pending: boolean;
}

export class ApiError extends Error {
	constructor(
		public status: number,
		message: string
	) {
		super(message);
	}
}

let onUnauthorized: () => void = () => {};
export function setUnauthorizedHandler(handler: () => void) {
	onUnauthorized = handler;
}

async function request<T>(method: string, path: string, body?: unknown): Promise<T> {
	const init: RequestInit = { method, credentials: 'same-origin' };
	if (body instanceof FormData) {
		init.body = body;
	} else if (body !== undefined) {
		init.headers = { 'Content-Type': 'application/json' };
		init.body = JSON.stringify(body);
	}
	let response: Response;
	try {
		response = await fetch(`/api${path}`, init);
	} catch {
		throw new ApiError(0, 'Cannot reach the server. Check your connection and try again.');
	}
	if (!response.ok) {
		const data = await response.json().catch(() => null);
		if (response.status === 401) onUnauthorized();
		throw new ApiError(response.status, data?.error ?? `Request failed (${response.status})`);
	}
	return response.json();
}

export const api = {
	get: <T>(path: string) => request<T>('GET', path),
	post: <T>(path: string, body?: unknown) => request<T>('POST', path, body ?? {}),
	put: <T>(path: string, body: unknown) => request<T>('PUT', path, body),
	delete: <T>(path: string) => request<T>('DELETE', path)
};

export interface FileEntry {
	message_id: number;
	idx: number;
	filename: string;
	size: number;
	kind: 'image' | 'pdf' | 'office' | 'other';
	date: number;
	thread_id: number;
	subject: string;
	from_name: string;
	from_addr: string;
}

export interface FileList {
	office_previews: boolean;
	files: FileEntry[];
}
