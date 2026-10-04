<script lang="ts">
	import { tick } from 'svelte';
	import { api, type Account, type Addr, type Category, type RecipientGroup, type Sender, type User } from '#lib/api.ts';
	import { app, categoryNames, classify, refreshAccounts, refreshCounts, refreshSettings } from '#lib/app.svelte.ts';
	import { clearOffline, offline, setOffline } from '#lib/offline.svelte.ts';
	import { swipeActions, type SwipeAction } from '#lib/swipe.ts';
	import Avatar from '#lib/components/Avatar.svelte';
	import ClassifyButtons from '#lib/components/ClassifyButtons.svelte';
	import SenderPicture from '#lib/components/SenderPicture.svelte';
	import { displayName, fullDate } from '#lib/format.ts';
	import { look, modes, setLook, themes } from '#lib/look.svelte.ts';

	// ---- Mail accounts ----
	const blank = () => ({
		id: null as number | null,
		label: '',
		address: '',
		display_name: '',
		imap_host: '',
		imap_port: 993,
		imap_security: 'tls',
		imap_username: '',
		smtp_host: '',
		smtp_port: 465,
		smtp_security: 'tls',
		smtp_username: '',
		password: '',
		inbox_folder: 'INBOX',
		junk_folder: '',
		sent_folder: '',
		trash_folder: '',
		archive_folder: '',
		feed_folder: '',
		delayed_folder: '',
		append_sent: true
	});
	let form = $state<ReturnType<typeof blank> | null>(null);
	let formError = $state('');
	let testResult = $state<{ ok: boolean; imap_error: string | null; smtp_error: string | null } | null>(null);
	let busy = $state(false);

	function edit(account: Account) {
		form = { ...blank(), ...account, password: '' };
		formError = '';
		testResult = null;
	}

	/** Most providers follow imap.<domain> / smtp.<domain>; a starting point the user can correct. */
	function guessServers() {
		if (!form) return;
		const domain = form.address.split('@')[1];
		if (!domain) return;
		form.imap_host ||= `imap.${domain}`;
		form.smtp_host ||= `smtp.${domain}`;
		form.imap_username ||= form.address;
		form.smtp_username ||= form.address;
	}

	async function test() {
		if (!form) return;
		busy = true;
		formError = '';
		try {
			const result = await api.post<{
				ok: boolean;
				imap_error: string | null;
				smtp_error: string | null;
				junk_folder: string | null;
				sent_folder: string | null;
				trash_folder: string | null;
				archive_folder: string | null;
			}>('/accounts/test', form);
			testResult = result;
			form.junk_folder ||= result.junk_folder ?? '';
			form.sent_folder ||= result.sent_folder ?? '';
			form.trash_folder ||= result.trash_folder ?? '';
			form.archive_folder ||= result.archive_folder ?? '';
		} catch (e) {
			formError = (e as Error).message;
		} finally {
			busy = false;
		}
	}

	async function saveAccount(event: SubmitEvent) {
		event.preventDefault();
		if (!form) return;
		busy = true;
		formError = '';
		try {
			if (form.id) await api.put(`/accounts/${form.id}`, form);
			else await api.post('/accounts', form);
			form = null;
			await refreshAccounts();
		} catch (e) {
			formError = (e as Error).message;
		} finally {
			busy = false;
		}
	}

	let removing = $state<number | null>(null);
	async function removeAccount(id: number) {
		await api.delete(`/accounts/${id}`);
		removing = null;
		await refreshAccounts();
	}

	$effect(() => {
		app.tick;
		refreshAccounts().catch(() => {});
	});

	// ---- Senders ----
	let senders = $state<Sender[]>([]);
	let filter = $state<Category | ''>('');
	let editingSender = $state<number | null>(null);
	/** Sender whose picture chooser is open. */
	let pictureOf = $state<number | null>(null);

	async function loadSenders() {
		senders = await api.get<Sender[]>(`/senders${filter ? `?category=${filter}` : ''}`);
	}
	$effect(() => {
		app.tick;
		filter;
		loadSenders().catch(() => {});
	});

	async function reclassify(sender: Sender, category: Category) {
		await classify(sender.id, category);
		editingSender = null;
	}

	// ---- Sliding a mail ----
	let swipeError = $state('');

	async function toggleSwipe(direction: 'left' | 'right', action: SwipeAction, on: boolean) {
		const next = { left: [...app.swipe.left], right: [...app.swipe.right] };
		next[direction] = on ? [...next[direction], action] : next[direction].filter((a) => a !== action);
		swipeError = '';
		try {
			const saved = await api.put<{ swipe_left: SwipeAction[]; swipe_right: SwipeAction[] }>('/settings', {
				swipe_left: next.left,
				swipe_right: next.right
			});
			app.swipe = { left: saved.swipe_left, right: saved.swipe_right };
		} catch (e) {
			swipeError = (e as Error).message;
		}
	}

	// ---- Backup, export, import ----
	let exportBox = $state('all');
	const exportBoxes = [
		['all', 'All mail'],
		['important', 'Home'],
		['flagged', 'Important'],
		['feed', 'Nice to know'],
		['archive', 'Archive'],
		['sent', 'Sent'],
		['junk', 'Junk'],
		['trash', 'Trash']
	];
	let portableBusy = $state(false);
	let portableError = $state('');
	let imported = $state<{
		senders: number;
		pictures: number;
		groups: number;
		saved_searches: number;
		notes: number;
		notes_without_mail: number;
		problems: string[];
	} | null>(null);

	/** Hands a text to the browser as a file to save. */
	function saveAs(name: string, text: string) {
		const link = document.createElement('a');
		link.href = URL.createObjectURL(new Blob([text], { type: 'application/json' }));
		link.download = name;
		link.click();
		setTimeout(() => URL.revokeObjectURL(link.href), 1000);
	}

	async function exportSettings() {
		portableError = '';
		portableBusy = true;
		try {
			const file = await api.get<Record<string, unknown>>('/export/settings');
			// What this browser keeps goes along: the look.
			file.browser = { look: { theme: look.theme, mode: look.mode } };
			saveAs(`portier-settings-${new Date().toISOString().slice(0, 10)}.json`, JSON.stringify(file, null, 1));
		} catch (e) {
			portableError = (e as Error).message;
		} finally {
			portableBusy = false;
		}
	}

	async function importSettings(event: Event) {
		const input = event.currentTarget as HTMLInputElement;
		const chosen = input.files?.[0];
		input.value = '';
		if (!chosen) return;
		portableError = '';
		imported = null;
		portableBusy = true;
		try {
			let file: { portier_settings?: number; browser?: { look?: { theme?: string; mode?: string } } };
			try {
				file = JSON.parse(await chosen.text());
			} catch {
				throw new Error('This is not a Portier settings file.');
			}
			if (file?.portier_settings !== 1) throw new Error('This is not a Portier settings file.');
			imported = await api.post('/import/settings', file);
			const wanted = file.browser?.look;
			const theme = themes.find((t) => t.id === wanted?.theme)?.id;
			const mode = modes.find((m) => m.id === wanted?.mode)?.id;
			if (theme || mode) setLook({ ...(theme ? { theme } : {}), ...(mode ? { mode } : {}) });
			groups = await api.get<RecipientGroup[]>('/groups');
			await Promise.all([refreshCounts(), refreshSettings()]);
			app.tick += 1;
		} catch (e) {
			portableError = (e as Error).message;
		} finally {
			portableBusy = false;
		}
	}

	// The whole installation, for the administrator.
	type BackupState = {
		dir: string;
		last_at: number | null;
		last_file: string | null;
		last_error: string | null;
		keep: number;
		data_dir: string;
	};
	let backup = $state<BackupState | null>(null);
	let backupDir = $state('');
	let backupError = $state('');
	let backupNote = $state('');
	let backupBusy = $state(false);

	$effect(() => {
		if (!app.user?.is_admin) return;
		api.get<BackupState>('/backup')
			.then((state) => {
				backup = state;
				backupDir = state.dir;
			})
			.catch(() => {});
	});

	async function saveBackupFolder(event: SubmitEvent) {
		event.preventDefault();
		backupError = backupNote = '';
		backupBusy = true;
		try {
			backup = await api.put<BackupState>('/backup', { dir: backupDir });
			backupDir = backup.dir;
			backupNote = backup.dir
				? 'Saved, and the first backup is written. From now on one is written every day.'
				: 'Saved. No backups are written by themselves.';
		} catch (e) {
			backupError = (e as Error).message;
			api.get<BackupState>('/backup')
				.then((state) => (backup = state))
				.catch(() => {});
		} finally {
			backupBusy = false;
		}
	}

	// ---- Groups of recipients ----
	let groups = $state<RecipientGroup[]>([]);
	/** The group being edited: its id, 0 for a new one, or null when the form is closed. */
	let groupId = $state<number | null>(null);
	let groupName = $state('');
	let groupMembers = $state('');
	let groupError = $state('');
	let groupBusy = $state(false);

	$effect(() => {
		api.get<RecipientGroup[]>('/groups')
			.then((list) => (groups = list))
			.catch(() => {});
	});

	function editGroup(group: RecipientGroup | null) {
		groupId = group?.id ?? 0;
		groupName = group?.name ?? '';
		// One address per line reads better than one long line.
		groupMembers = group?.members.split(', ').join('\n') ?? '';
		groupError = '';
		memberHits = [];
	}

	// Known addresses that match what is being typed in the list of members.
	let memberField: HTMLTextAreaElement | undefined = $state();
	let memberHits = $state<Addr[]>([]);
	let memberChosen = $state(-1);
	let memberAsked = 0;

	/** Where the address being typed starts: after the last comma, semicolon or line break before the cursor. */
	function memberStart(text: string, cursor: number) {
		const before = text.slice(0, cursor);
		return Math.max(before.lastIndexOf(','), before.lastIndexOf(';'), before.lastIndexOf('\n')) + 1;
	}

	async function suggestMembers() {
		if (!memberField) return;
		const cursor = memberField.selectionStart;
		const term = groupMembers.slice(memberStart(groupMembers, cursor), cursor).trim();
		const asked = ++memberAsked;
		const found = term.length >= 2 ? await api.get<Addr[]>(`/contacts?q=${encodeURIComponent(term)}`).catch(() => []) : [];
		// An answer to something typed earlier must not replace a newer one.
		if (asked !== memberAsked) return;
		const taken = new Set(groupMembers.toLowerCase().split(/[,;\n]/).map((part) => part.trim()));
		// Addresses only: a group is not put into a group, and what is in the list is not offered again.
		memberHits = found.filter((hit) => !hit.group && !taken.has(hit.address.toLowerCase()));
		memberChosen = -1;
	}

	/** Puts the address in place of what was typed, on a line of its own, ready for the next one. */
	function takeMember(contact: Addr) {
		if (!memberField) return;
		const cursor = memberField.selectionStart;
		const start = memberStart(groupMembers, cursor);
		const lead = start > 0 && groupMembers[start - 1] !== '\n' ? ' ' : '';
		const rest = groupMembers.slice(cursor).replace(/^[^\S\n]*\n?/, '');
		const head = `${groupMembers.slice(0, start)}${lead}${contact.address}\n`;
		groupMembers = head + rest;
		memberHits = [];
		memberChosen = -1;
		const field = memberField;
		field.focus();
		// Once the value has reached the field, the cursor goes behind the new address.
		tick().then(() => field.setSelectionRange(head.length, head.length));
	}

	/** Arrow keys walk through the suggestions, Enter or Tab takes the picked one, Escape closes them. */
	function memberKeydown(event: KeyboardEvent) {
		if (!memberHits.length || event.ctrlKey || event.metaKey || event.altKey) return;
		if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
			event.preventDefault();
			const step = event.key === 'ArrowDown' ? 1 : -1;
			memberChosen =
				memberChosen < 0 && step < 0 ? memberHits.length - 1 : (memberChosen + step + memberHits.length) % memberHits.length;
			document.getElementById(`member-suggestion-${memberChosen}`)?.scrollIntoView({ block: 'nearest' });
		} else if ((event.key === 'Enter' || event.key === 'Tab') && memberChosen >= 0) {
			event.preventDefault();
			takeMember(memberHits[memberChosen]);
		} else if (event.key === 'Escape') {
			event.preventDefault();
			memberHits = [];
			memberChosen = -1;
		}
	}

	async function saveGroup(event: SubmitEvent) {
		event.preventDefault();
		groupError = '';
		groupBusy = true;
		try {
			const body = { name: groupName, members: groupMembers };
			if (groupId) await api.put(`/groups/${groupId}`, body);
			else await api.post('/groups', body);
			groups = await api.get<RecipientGroup[]>('/groups');
			groupId = null;
		} catch (e) {
			groupError = (e as Error).message;
		} finally {
			groupBusy = false;
		}
	}

	async function deleteGroup(group: RecipientGroup) {
		groupError = '';
		try {
			await api.delete(`/groups/${group.id}`);
			groups = groups.filter((g) => g.id !== group.id);
			if (groupId === group.id) groupId = null;
		} catch (e) {
			groupError = (e as Error).message;
		}
	}

	// ---- Mail on this device ----
	let offlineBusy = $state(false);

	async function toggleOffline(on: boolean) {
		offlineBusy = true;
		try {
			await setOffline(on);
		} finally {
			offlineBusy = false;
		}
	}

	// ---- Summaries by a local model ----
	let ai = $state({ on: false, reads_pdf: true, url: '', model: '', language: 'English' });
	let aiModels = $state<string[]>([]);
	let aiError = $state('');
	let aiNote = $state('');
	let aiBusy = $state(false);

	$effect(() => {
		api.get<typeof ai>('/settings/ai')
			.then((saved) => {
				ai = { ...ai, ...saved };
				if (saved.model) aiModels = [saved.model];
			})
			.catch(() => {});
	});

	/** Asks the Ollama at the address which models it has; also shows that the address works. */
	async function findModels() {
		aiError = aiNote = '';
		aiBusy = true;
		try {
			const found = await api.post<{ models: string[] }>('/settings/ai/models', { url: ai.url });
			aiModels = found.models;
			if (!found.models.includes(ai.model)) ai.model = found.models[0] ?? '';
			aiNote = found.models.length
				? `Ollama answers and offers ${found.models.length} ${found.models.length === 1 ? 'model' : 'models'}.`
				: 'Ollama answers but has no model yet. Pull one with "ollama pull".';
		} catch (e) {
			aiError = (e as Error).message;
		} finally {
			aiBusy = false;
		}
	}

	async function saveAi(event: SubmitEvent) {
		event.preventDefault();
		aiError = aiNote = '';
		aiBusy = true;
		try {
			await api.put('/settings/ai', { url: ai.url, model: ai.model, language: ai.language });
			ai.on = !!ai.url.trim() && !!ai.model;
			aiNote = ai.on
				? 'Saved. New unseen mail in Home is summarized from now on.'
				: 'Saved. Summaries are off.';
		} catch (e) {
			aiError = (e as Error).message;
		} finally {
			aiBusy = false;
		}
	}

	// ---- Automatic archive ----
	let archiveOn = $state(app.autoArchiveWeeks > 0);
	let archiveWeeks = $state(app.autoArchiveWeeks || 8);
	let archiveDue = $state<number | null>(null);
	let archiveError = $state('');
	let archiveNote = $state('');
	let archiveBusy = $state(false);
	const archiveChanged = $derived((archiveOn ? archiveWeeks : 0) !== app.autoArchiveWeeks);

	// The saved setting arrives a moment after the page; show it once it is there.
	$effect(() => {
		const saved = app.autoArchiveWeeks;
		archiveOn = saved > 0;
		if (saved > 0) archiveWeeks = saved;
	});

	// Before saving, say how much a new setting would archive right away.
	$effect(() => {
		const weeks = archiveOn ? archiveWeeks : 0;
		archiveDue = null;
		if (!archiveChanged || weeks < 1 || weeks > 520) return;
		const timer = setTimeout(() => {
			api.get<{ count: number }>(`/settings/auto-archive/preview?weeks=${weeks}`)
				.then((result) => (archiveDue = result.count))
				.catch(() => {});
		}, 300);
		return () => clearTimeout(timer);
	});

	async function saveArchive(event: SubmitEvent) {
		event.preventDefault();
		archiveBusy = true;
		archiveError = archiveNote = '';
		try {
			const result = await api.put<{ auto_archive_weeks: number; archived: number }>('/settings/auto-archive', {
				weeks: archiveOn ? archiveWeeks : 0
			});
			app.autoArchiveWeeks = result.auto_archive_weeks;
			app.tick += 1;
			archiveNote =
				result.auto_archive_weeks === 0
					? 'Automatic archive is off'
					: `Saved. ${result.archived === 0 ? 'Nothing was due' : `Archived ${result.archived} ${result.archived === 1 ? 'mail' : 'mails'}`} just now.`;
		} catch (e) {
			archiveError = (e as Error).message;
		} finally {
			archiveBusy = false;
		}
	}

	// ---- Users (admin) ----
	let users = $state<User[]>([]);
	let newUser = $state({ email: '', password: '' });
	let userError = $state('');

	async function loadUsers() {
		if (app.user?.is_admin) users = await api.get<User[]>('/users');
	}
	$effect(() => {
		loadUsers().catch(() => {});
	});

	async function addUser(event: SubmitEvent) {
		event.preventDefault();
		userError = '';
		try {
			await api.post('/users', newUser);
			newUser = { email: '', password: '' };
			await loadUsers();
		} catch (e) {
			userError = (e as Error).message;
		}
	}

	let removingUser = $state<number | null>(null);
	async function removeUser(id: number) {
		await api.delete(`/users/${id}`);
		removingUser = null;
		await loadUsers();
	}

	// ---- Own password ----
	let passwords = $state({ current: '', new: '' });
	let passwordNote = $state('');
	let passwordError = $state('');

	async function changePassword(event: SubmitEvent) {
		event.preventDefault();
		passwordNote = passwordError = '';
		try {
			await api.post('/password', passwords);
			passwords = { current: '', new: '' };
			passwordNote = 'Password changed';
		} catch (e) {
			passwordError = (e as Error).message;
		}
	}
</script>

<div class="page-head measure"><h1>Settings</h1></div>

<nav class="jump" aria-label="Sections of this page">
	<a class="btn small" href="#accounts">Mail accounts</a>
	<a class="btn small" href="#senders">Senders</a>
	<a class="btn small" href="#look">Look</a>
	<a class="btn small" href="#sliding">Sliding</a>
	<a class="btn small" href="#archive">Automatic archive</a>
	<a class="btn small" href="#summaries">Summaries</a>
	<a class="btn small" href="#groups">Groups</a>
	<a class="btn small" href="#portable">Backup and export</a>
	<a class="btn small" href="#device">Mail on this device</a>
	{#if app.user?.is_admin}<a class="btn small" href="#users">Users</a>{/if}
	<a class="btn small" href="#password">Password</a>
</nav>

<section id="accounts">
	<h2>Mail accounts</h2>
	{#if app.accounts.length === 0 && !form}
		<p class="muted">No account connected yet. Portier works with any mailbox that offers IMAP and SMTP.</p>
	{/if}
	{#if app.accounts.length}
		<ul class="sheet list">
			{#each app.accounts as account (account.id)}
				<li>
					<div class="grow">
						<strong>{account.label}</strong>
						<span class="muted">{account.address}</span>
						{#if account.last_error}
							<span class="error">Not syncing: {account.last_error}</span>
						{:else if account.last_sync_at}
							<span class="muted small">Last checked {fullDate(account.last_sync_at)}</span>
						{:else}
							<span class="muted small">Fetching mail for the first time</span>
						{/if}
					</div>
					{#if removing === account.id}
						<span class="small">Remove this account and its mail from Portier? The mailbox itself is untouched.</span>
						<button class="btn small danger" onclick={() => removeAccount(account.id)}>Remove account</button>
						<button class="btn small" onclick={() => (removing = null)}>Keep</button>
					{:else}
						<button class="btn small" onclick={() => edit(account)}>Edit</button>
						<button class="btn small quiet danger" onclick={() => (removing = account.id)}>Remove</button>
					{/if}
				</li>
			{/each}
		</ul>
	{/if}

	{#if form}
		<form class="sheet account" onsubmit={saveAccount}>
			<h3>{form.id ? 'Edit mail account' : 'Add a mail account'}</h3>
			<div class="grid">
				<label class="field">
					Email address
					<input type="email" bind:value={form.address} onblur={guessServers} required />
				</label>
				<label class="field">
					Your name, as recipients see it
					<input bind:value={form.display_name} />
				</label>
				<label class="field">
					Name for this account
					<input bind:value={form.label} placeholder="Work, Private" />
				</label>
				<label class="field">
					Password
					<input
						type="password"
						bind:value={form.password}
						required={!form.id}
						autocomplete="new-password"
						placeholder={form.id ? 'Leave empty to keep the current one' : ''}
					/>
				</label>
			</div>
			<p class="muted small">
				Gmail, iCloud and others need an app password created in their security settings, not your normal password.
			</p>

			<h4>Incoming mail (IMAP)</h4>
			<div class="grid server">
				<label class="field">Server <input bind:value={form.imap_host} required /></label>
				<label class="field">Port <input type="number" bind:value={form.imap_port} required /></label>
				<label class="field">
					Encryption
					<select bind:value={form.imap_security}>
						<option value="tls">TLS</option>
						<option value="starttls">STARTTLS</option>
						<option value="none">None</option>
					</select>
				</label>
				<label class="field">User name <input bind:value={form.imap_username} required /></label>
			</div>

			<h4>Outgoing mail (SMTP)</h4>
			<div class="grid server">
				<label class="field">Server <input bind:value={form.smtp_host} required /></label>
				<label class="field">Port <input type="number" bind:value={form.smtp_port} required /></label>
				<label class="field">
					Encryption
					<select bind:value={form.smtp_security}>
						<option value="tls">TLS</option>
						<option value="starttls">STARTTLS</option>
						<option value="none">None</option>
					</select>
				</label>
				<label class="field">User name <input bind:value={form.smtp_username} required /></label>
			</div>

			<h4>Folders on the mail server</h4>
			<div class="grid">
				<label class="field">Inbox folder <input bind:value={form.inbox_folder} /></label>
				<label class="field">Junk folder <input bind:value={form.junk_folder} placeholder="Found automatically" /></label>
				<label class="field">Sent folder <input bind:value={form.sent_folder} placeholder="Found automatically" /></label>
				<label class="field">Trash folder <input bind:value={form.trash_folder} placeholder="Found automatically" /></label>
				<label class="field">
					Archive folder <input bind:value={form.archive_folder} placeholder="Found automatically" />
				</label>
				<label class="field">
					Nice to know folder <input bind:value={form.feed_folder} placeholder="None: stays in the inbox" />
				</label>
				<label class="field">
					Delayed folder <input bind:value={form.delayed_folder} placeholder="None: stays where it is" />
				</label>
			</div>
			<p class="muted note">
				With a Nice to know folder, e.g. <em>Nice to know</em>, mail from senders you filed under Nice to know is
				moved there on the mail server, out of the inbox. With a Delayed folder, e.g. <em>Delayed</em>, a
				conversation you delay waits there and moves back to the inbox when it returns. Both folders are created
				if they are missing, and in Portier the mail shows as before. Before you empty one of these fields
				again, move its mail back to the inbox with another mail program: Portier then no longer looks into
				that folder.
			</p>
			<label class="check">
				<input type="checkbox" bind:checked={form.append_sent} />
				Save a copy of sent mail in the Sent folder (turn off for Gmail, which does this itself)
			</label>

			{#if testResult}
				<div class="test" role="status">
					<span class={testResult.imap_error ? 'error' : 'ok'}>
						Incoming mail: {testResult.imap_error ?? 'works'}
					</span>
					<span class={testResult.smtp_error ? 'error' : 'ok'}>
						Outgoing mail: {testResult.smtp_error ?? 'works'}
					</span>
				</div>
			{/if}
			{#if formError}<p class="error" role="alert">{formError}</p>{/if}

			<div class="actions">
				<button class="btn primary" disabled={busy}>{form.id ? 'Save account' : 'Add account'}</button>
				<button type="button" class="btn" disabled={busy} onclick={test}>Test connection</button>
				<button type="button" class="btn quiet" onclick={() => (form = null)}>Cancel</button>
			</div>
		</form>
	{:else}
		<p><button class="btn primary" onclick={() => { form = blank(); testResult = null; formError = ''; }}>Add a mail account</button></p>
	{/if}
</section>

<section id="senders">
	<h2>Senders you decided on</h2>
	<p class="muted">Changing a sender moves all of their mail, old and new. Leaving Junk brings back the last 90 days.</p>
	<div class="filter">
		<button class="btn small" aria-pressed={filter === ''} onclick={() => (filter = '')}>All</button>
		{#each ['important', 'feed', 'junk'] as const as category}
			<button class="btn small" aria-pressed={filter === category} onclick={() => (filter = category)}>
				{categoryNames[category]}
			</button>
		{/each}
	</div>
	{#if senders.length === 0}
		<p class="muted">No senders here yet. Decisions you make in the Screener are listed here.</p>
	{:else}
		<ul class="sheet list">
			{#each senders as sender (sender.id)}
				<li class="sender">
					<Avatar name={displayName(sender.display_name, sender.address)} seed={sender.address} address={sender.address} size={36} />
					<div class="grow">
						<strong>{displayName(sender.display_name, sender.address)}</strong>
						<span class="muted">{sender.address}, {sender.count} {sender.count === 1 ? 'message' : 'messages'}</span>
					</div>
					{#if editingSender === sender.id}
						<ClassifyButtons small current={sender.category} onpick={(c) => reclassify(sender, c)} />
					{:else}
						{#if sender.category}<span class="tag {sender.category}">{categoryNames[sender.category]}</span>{/if}
						<button class="btn small quiet" onclick={() => (editingSender = sender.id)}>Change</button>
					{/if}
					<button
						class="btn small quiet"
						aria-expanded={pictureOf === sender.id}
						onclick={() => (pictureOf = pictureOf === sender.id ? null : sender.id)}
					>
						Picture
					</button>
					{#if pictureOf === sender.id}
						<div class="picture-panel">
							<SenderPicture
								senderId={sender.id}
								address={sender.address}
								name={displayName(sender.display_name, sender.address)}
								hasPicture={sender.has_picture}
								onchange={(has) => (sender.has_picture = has)}
							/>
						</div>
					{/if}
				</li>
			{/each}
		</ul>
	{/if}
</section>

<section id="look">
	<h2>Look</h2>
	<p class="muted">Colours of Portier in this browser. Mails that bring their own design keep their white page.</p>
	<fieldset class="choices">
		<legend>Light or dark</legend>
		{#each modes as mode}
			<label class="check">
				<input type="radio" name="mode" checked={look.mode === mode.id} onchange={() => setLook({ mode: mode.id })} />
				{mode.name}
			</label>
		{/each}
	</fieldset>
	<fieldset class="themes">
		<legend>Theme</legend>
		{#each themes as theme}
			<!-- Each swatch is painted in its own theme, in the light or dark that is on now. -->
			<label class="theme" data-theme={theme.id} data-mode={look.dark ? 'dark' : 'light'}>
				<input type="radio" name="theme" checked={look.theme === theme.id} onchange={() => setLook({ theme: theme.id })} />
				<span class="swatch" aria-hidden="true">
					<span class="line strong"></span>
					<span class="line"></span>
					<span class="dot"></span>
				</span>
				<strong>{theme.name}</strong>
				<span class="hint">{theme.hint}</span>
			</label>
		{/each}
	</fieldset>
</section>

<section id="sliding">
	<h2>Sliding a mail</h2>
	<p class="muted">
		In the mail lists, slide a row left or right with a finger or the mouse. With one action chosen, sliding far
		enough performs it. With several, sliding shows them as buttons. With none, that direction does nothing.
	</p>
	<div class="swipe-grid">
		{#each [{ key: 'right', title: 'Slide right' }, { key: 'left', title: 'Slide left' }] as const as direction}
			<fieldset>
				<legend>{direction.title}</legend>
				{#each swipeActions as action}
					<label class="check">
						<input
							type="checkbox"
							checked={app.swipe[direction.key].includes(action.id)}
							onchange={(event) => toggleSwipe(direction.key, action.id, event.currentTarget.checked)}
						/>
						{action.name}
					</label>
				{/each}
			</fieldset>
		{/each}
	</div>
	{#if swipeError}<p class="error" role="alert">{swipeError}</p>{/if}
</section>

<section id="archive">
	<h2>Automatic archive</h2>
	<p class="muted">
		Seen conversations in Home move to the Archive folder by themselves once their newest mail
		is older than the age below. Unseen, Important and delayed conversations stay, and so do the other lists.
		Checked once an hour.
	</p>
	<form class="auto" onsubmit={saveArchive}>
		<label class="check">
			<input type="checkbox" bind:checked={archiveOn} />
			Archive seen mail automatically
		</label>
		<label class="age">
			when older than
			<input type="number" min="1" max="520" bind:value={archiveWeeks} disabled={!archiveOn} aria-label="Age in weeks" />
			weeks
		</label>
		<button class="btn" disabled={archiveBusy || !archiveChanged}>Save</button>
	</form>
	{#if archiveChanged && archiveOn && archiveDue !== null}
		<p class="due" role="status">
			{#if archiveDue === 0}
				Nothing in Seen is that old at the moment.
			{:else}
				Saving archives {archiveDue}
				{archiveDue === 1 ? 'conversation' : 'conversations'} right away.
			{/if}
		</p>
	{/if}
	{#if archiveError}<p class="error" role="alert">{archiveError}</p>{/if}
	{#if archiveNote}<p class="ok" role="status">{archiveNote}</p>{/if}
</section>

<section id="groups">
	<h2>Groups</h2>
	<p class="muted">
		A group is a name for several addresses. When writing a mail, type the name in the To field and pick the
		group: its addresses are filled in, and the mail goes to each of them as usual.
	</p>
	{#if groups.length}
		<ul class="groups">
			{#each groups as group (group.id)}
				{@const count = group.members.split(', ').length}
				<li>
					<div>
						<strong>{group.name}</strong>
						<span class="muted">{count} {count === 1 ? 'address' : 'addresses'}: {group.members}</span>
					</div>
					<button class="btn small" onclick={() => editGroup(group)}>Edit</button>
					<button class="btn small danger" onclick={() => deleteGroup(group)}>Delete</button>
				</li>
			{/each}
		</ul>
	{/if}
	{#if groupId === null}
		<p><button class="btn" onclick={() => editGroup(null)}>New group</button></p>
	{:else}
		<form class="group" onsubmit={saveGroup}>
			<label class="field">
				Name
				<input bind:value={groupName} maxlength="40" placeholder="e.g. Board" autocomplete="off" />
			</label>
			<div class="members">
				<label class="field" for="group-members">Addresses, one per line or separated by commas</label>
				<textarea
					id="group-members"
					class="input"
					rows="5"
					role="combobox"
					aria-autocomplete="list"
					aria-expanded={memberHits.length > 0}
					aria-controls="member-suggestions"
					aria-activedescendant={memberChosen >= 0 ? `member-suggestion-${memberChosen}` : undefined}
					bind:this={memberField}
					bind:value={groupMembers}
					oninput={suggestMembers}
					onkeydown={memberKeydown}
					onblur={() => setTimeout(() => (memberHits = []), 150)}
					placeholder="Type a name or an address"
					spellcheck="false"
					autocomplete="off"
					autocapitalize="off"
				></textarea>
				{#if memberHits.length}
					<ul class="suggestions sheet" id="member-suggestions" role="listbox" aria-label="Known addresses">
						{#each memberHits as contact, index}
							<li role="presentation">
								<button
									type="button"
									id="member-suggestion-{index}"
									role="option"
									aria-selected={index === memberChosen}
									tabindex="-1"
									onclick={() => takeMember(contact)}
								>
									<strong>{displayName(contact.name, contact.address)}</strong>
									<span class="muted">{contact.address}</span>
								</button>
							</li>
						{/each}
					</ul>
				{/if}
			</div>
			<div class="actions">
				<button class="btn primary" disabled={groupBusy}>{groupId ? 'Save group' : 'Add group'}</button>
				<button type="button" class="btn" onclick={() => (groupId = null)}>Cancel</button>
			</div>
		</form>
	{/if}
	{#if groupError}<p class="error" role="alert">{groupError}</p>{/if}
</section>

<section id="portable">
	<h2>Backup and export</h2>

	<h3>Your settings</h3>
	<p class="muted">
		One file with what you set up in Portier: where each sender's mail goes, sender pictures, groups, saved
		searches, notes on mails, slide actions, automatic archive and the look. Mail and mail accounts are not in
		it. Importing sets what the file names and leaves everything else as it is.
	</p>
	<div class="actions">
		<button class="btn" disabled={portableBusy} onclick={exportSettings}>Export settings</button>
		<label class="btn" class:disabled={portableBusy}>
			Import settings
			<input type="file" accept="application/json,.json" hidden disabled={portableBusy} onchange={importSettings} />
		</label>
	</div>
	{#if portableError}<p class="error" role="alert">{portableError}</p>{/if}
	{#if imported}
		<p class="ok" role="status">
			Imported: {imported.senders} senders, {imported.pictures} pictures, {imported.groups} groups,
			{imported.saved_searches} saved searches, {imported.notes} notes.
		</p>
		{#if imported.notes_without_mail}
			<p class="muted note">
				{imported.notes_without_mail}
				{imported.notes_without_mail === 1 ? 'note was' : 'notes were'} left out: the mail they belong to is not
				here (yet). Import again once it is.
			</p>
		{/if}
		{#if imported.problems.length}
			<p class="error">Not taken over: {imported.problems.join('; ')}</p>
		{/if}
	{/if}

	<h3>Mail as files</h3>
	<p class="muted">
		Your mail as an mbox file, which Thunderbird, Apple Mail and other mail programs open and import. A single
		mail or conversation can be saved from the mail itself ("Save as file"), a selection from the selection bar.
	</p>
	<div class="actions">
		<select bind:value={exportBox} aria-label="Which mail">
			{#each exportBoxes as [id, name]}<option value={id}>{name}</option>{/each}
		</select>
		<a class="btn" href="/api/export/mail?box={exportBox}" download>Export as mbox file</a>
	</div>

	{#if app.user?.is_admin}
		<h3>Whole installation</h3>
		<p class="muted">
			A backup holds everything of every user: the database, the stored mail, and the key that protects the mail
			passwords. Keep it as safe as the server itself. It is put back with the server stopped:
			<code>portier restore FILE</code>; <code>portier backup</code> writes one from the command line.
		</p>
		<div class="actions">
			<a class="btn" href="/api/backup/download" download>Download a backup now</a>
		</div>
		<form class="daily" onsubmit={saveBackupFolder}>
			<label class="field">
				Folder for a daily backup (on the server; empty for none)
				<input bind:value={backupDir} placeholder="/path/to/backups" autocomplete="off" spellcheck="false" />
			</label>
			<button class="btn primary" disabled={backupBusy || backupDir.trim() === (backup?.dir ?? '')}>Save</button>
		</form>
		{#if backup?.dir}
			<p class="muted note">
				One backup a day, the newest {backup.keep} are kept.
				{#if backup.last_at}Last one: {fullDate(backup.last_at)}{backup.last_file ? `, ${backup.last_file}` : ''}.{/if}
			</p>
		{/if}
		{#if backup?.last_error}<p class="error" role="alert">The last backup failed: {backup.last_error}</p>{/if}
		{#if backupError}<p class="error" role="alert">{backupError}</p>{/if}
		{#if backupNote}<p class="ok" role="status">{backupNote}</p>{/if}
	{/if}
</section>

<section id="device">
	<h2>Mail on this device</h2>
	<p class="muted">
		Keeps the mail you read, and the newest 50 conversations of Home, in this browser, so that they can be read
		when the server cannot be reached. Changing anything still needs a connection, and attachments are not kept.
		The setting is for this device only.
	</p>
	{#if offline.possible}
		<label class="check">
			<input
				type="checkbox"
				checked={offline.enabled}
				disabled={offlineBusy}
				onchange={(event) => toggleOffline(event.currentTarget.checked)}
			/>
			Keep mail on this device
		</label>
		{#if offline.enabled}
			<p class="kept">
				{offline.mails}
				{offline.mails === 1 ? 'conversation is' : 'conversations are'} kept here, at most 200 and none longer than
				two weeks.
				<button class="btn small" disabled={offlineBusy || offline.mails === 0} onclick={clearOffline}>
					Delete kept mail
				</button>
			</p>
		{/if}
		<p class="muted note">
			Kept mail is not encrypted: whoever can use this browser profile can read it without signing in. It is
			deleted when you sign out, when your session ends, and when you turn this off.
		</p>
	{:else}
		<p>This browser cannot keep mail: it needs an https address (or localhost) and a current browser.</p>
	{/if}
</section>

<section id="summaries">
	<h2>Summaries</h2>
	<p class="muted">
		A language model running on your own machines (Ollama) writes a sentence or two about each new, unseen mail
		in Home and about its text and PDF attachments. The briefing shows above the Unseen area. Mail from senders
		you have not let into Home is never given to the model, and nothing leaves for a service on the internet.
	</p>
	{#if app.user?.is_admin}
		<form class="ai" onsubmit={saveAi}>
			<label class="field">
				Address of Ollama
				<input bind:value={ai.url} placeholder="http://127.0.0.1:11434" autocomplete="off" spellcheck="false" />
			</label>
			<button type="button" class="btn" onclick={findModels} disabled={aiBusy || !ai.url.trim()}>Find models</button>
			<label class="field">
				Model
				<select bind:value={ai.model} disabled={!aiModels.length}>
					{#each aiModels as name}<option value={name}>{name}</option>{/each}
					{#if !aiModels.length}<option value="">Find models first</option>{/if}
				</select>
			</label>
			<label class="field">
				Language of the summaries
				<input bind:value={ai.language} placeholder="English" />
			</label>
			<button class="btn primary" disabled={aiBusy}>Save</button>
		</form>
		<p class="muted note">
			An empty address turns summaries off. A small, fast model is enough; each mail is summarized once, when it
			arrives.
			{#if !ai.reads_pdf}
				PDF attachments are left out on this server: the program pdftotext (poppler) is not installed.
			{/if}
		</p>
		{#if aiError}<p class="error" role="alert">{aiError}</p>{/if}
		{#if aiNote}<p class="ok" role="status">{aiNote}</p>{/if}
	{:else}
		<p>{ai.on ? 'Summaries are on.' : 'Summaries are off.'} The administrator of this Portier sets the model.</p>
	{/if}
</section>

{#if app.user?.is_admin}
	<section id="users">
		<h2>Users</h2>
		<p class="muted">Each user has their own mail accounts and their own sender decisions.</p>
		<ul class="sheet list">
			{#each users as user (user.id)}
				<li>
					<div class="grow">
						<strong>{user.email}</strong>
						{#if user.is_admin}<span class="muted">Manages this installation</span>{/if}
					</div>
					{#if user.id !== app.user.id}
						{#if removingUser === user.id}
							<span class="small">Delete this user with all their accounts and decisions?</span>
							<button class="btn small danger" onclick={() => removeUser(user.id)}>Delete user</button>
							<button class="btn small" onclick={() => (removingUser = null)}>Keep</button>
						{:else}
							<button class="btn small quiet danger" onclick={() => (removingUser = user.id)}>Delete</button>
						{/if}
					{/if}
				</li>
			{/each}
		</ul>
		<form class="inline" onsubmit={addUser}>
			<label class="field">Email <input type="email" bind:value={newUser.email} required /></label>
			<label class="field">
				Password
				<input type="password" bind:value={newUser.password} required minlength="8" autocomplete="new-password" />
			</label>
			<button class="btn">Add user</button>
		</form>
		{#if userError}<p class="error" role="alert">{userError}</p>{/if}
	</section>
{/if}

<section id="password">
	<h2>Your password</h2>
	<form class="inline" onsubmit={changePassword}>
		<label class="field">
			Current password
			<input type="password" bind:value={passwords.current} required autocomplete="current-password" />
		</label>
		<label class="field">
			New password
			<input type="password" bind:value={passwords.new} required minlength="8" autocomplete="new-password" />
		</label>
		<button class="btn">Change password</button>
	</form>
	{#if passwordError}<p class="error" role="alert">{passwordError}</p>{/if}
	{#if passwordNote}<p class="ok" role="status">{passwordNote}</p>{/if}
</section>

<style>
	/* Forms stay at a readable width instead of stretching across the window. */
	:global(main:has(> .measure)) {
		max-width: var(--column);
	}
	section {
		margin-bottom: 3rem;
		/* Jump links must not park the heading under the fixed top bar. */
		scroll-margin-top: 5rem;
	}
	.jump {
		display: flex;
		flex-wrap: wrap;
		gap: 0.4rem;
		margin-bottom: 2rem;
	}
	section > h2 {
		margin-bottom: 0.6rem;
	}
	/* The picture chooser of a sender opens below its row. */
	li.sender {
		flex-wrap: wrap;
	}
	.picture-panel {
		flex-basis: 100%;
		padding: 0.3rem 0 0.5rem;
	}
	.choices,
	.themes {
		border: 0;
		margin: 1rem 0 0;
		padding: 0;
		background: none;
	}
	.choices legend,
	.themes legend {
		padding: 0;
		margin-bottom: 0.4rem;
		font-weight: 600;
		font-size: 0.9rem;
	}
	.choices {
		display: flex;
		flex-wrap: wrap;
		gap: 0.3rem 1.25rem;
	}
	.themes {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(9.5rem, 1fr));
		gap: 0.75rem;
	}
	/* A swatch card carries its theme's tokens itself (data-theme on the label), so the colours
	   inside are that theme's, whatever the page is set to. */
	.theme {
		position: relative;
		display: grid;
		gap: 0.15rem;
		padding: 0.6rem;
		border: 1px solid var(--line);
		border-radius: var(--radius);
		background: var(--surface);
		color: var(--ink);
		cursor: pointer;
	}
	.theme:has(input:checked) {
		outline: 3px solid var(--important);
		outline-offset: 1px;
	}
	.theme:has(input:focus-visible) {
		outline: 3px solid var(--signal);
		outline-offset: 2px;
	}
	.theme input {
		position: absolute;
		opacity: 0;
		pointer-events: none;
	}
	.swatch {
		display: grid;
		grid-template-columns: 1fr auto;
		gap: 0.35rem 0.5rem;
		align-items: center;
		padding: 0.7rem;
		margin-bottom: 0.4rem;
		border-radius: 9px;
		background: var(--paper);
		border: 1px solid var(--line);
	}
	.swatch .line {
		grid-column: 1;
		height: 0.4rem;
		width: 60%;
		border-radius: 999px;
		background: var(--ink-soft);
	}
	.swatch .line.strong {
		width: 85%;
		background: var(--ink);
	}
	.swatch .dot {
		grid-column: 2;
		grid-row: 1 / span 2;
		width: 1.5rem;
		height: 1.5rem;
		border-radius: 999px;
		background: var(--important);
	}
	.theme .hint {
		font-size: 0.85rem;
		color: var(--ink-soft);
	}
	h3 {
		font-size: 1.15rem;
		margin-bottom: 1rem;
	}
	h4 {
		margin: 1.5rem 0 0.6rem;
		font: 700 1rem var(--display);
	}
	.list {
		list-style: none;
		margin: 0.75rem 0;
		padding: 0;
	}
	.list li {
		display: flex;
		align-items: center;
		flex-wrap: wrap;
		gap: 0.6rem;
		padding: 0.8rem 1rem;
	}
	.list li + li {
		border-top: 1px solid var(--line);
	}
	.grow {
		display: grid;
		flex: 1;
		min-width: 12rem;
	}
	.small {
		font-size: 0.875rem;
	}
	.account {
		padding: 1.4rem;
		margin: 1rem 0;
	}
	.grid {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(13rem, 1fr));
		gap: 0.9rem;
	}
	.grid.server {
		grid-template-columns: 2fr 5rem 8rem 2fr;
	}
	@media (max-width: 40rem) {
		.grid.server {
			grid-template-columns: 1fr 1fr;
		}
	}
	#portable h3 {
		margin: 1.4rem 0 0.3rem;
		font: 600 1rem var(--body);
		letter-spacing: 0;
	}
	#portable .actions {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 0.6rem;
		margin-top: 0.6rem;
	}
	#portable select {
		padding: 0.5rem 0.7rem;
		border: 1px solid var(--line);
		border-radius: 10px;
		background: var(--surface);
		color: inherit;
		font: inherit;
	}
	#portable label.btn {
		cursor: pointer;
	}
	#portable label.btn.disabled {
		opacity: 0.55;
		pointer-events: none;
	}
	#portable code {
		font-size: 0.9em;
	}
	.daily {
		display: grid;
		grid-template-columns: minmax(0, 1fr) auto;
		gap: 0.75rem;
		align-items: end;
		max-width: 40rem;
		margin-top: 0.9rem;
	}
	.groups {
		list-style: none;
		margin: 0.75rem 0;
		padding: 0;
	}
	.groups li {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		padding: 0.55rem 0;
		border-top: 1px solid var(--line);
	}
	.groups li div {
		flex: 1;
		min-width: 0;
		display: grid;
	}
	.groups li span {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-size: 0.9rem;
	}
	.group {
		display: grid;
		gap: 0.75rem;
		max-width: 34rem;
		margin-top: 0.75rem;
	}
	.group textarea {
		resize: vertical;
		font: inherit;
	}
	.members {
		position: relative;
		display: grid;
		gap: 0.25rem;
	}
	.suggestions {
		position: absolute;
		top: 100%;
		left: 0;
		right: 0;
		z-index: 5;
		list-style: none;
		margin: 0;
		padding: 0.3rem;
	}
	.suggestions button {
		display: flex;
		gap: 0.6rem;
		width: 100%;
		padding: 0.45rem 0.6rem;
		border: 0;
		border-radius: 8px;
		background: none;
		text-align: left;
		cursor: pointer;
	}
	.suggestions button:hover {
		background: var(--paper);
	}
	.suggestions button[aria-selected='true'] {
		background: color-mix(in srgb, var(--important) 14%, var(--surface));
	}
	.kept {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 0.5rem 0.9rem;
	}
	.ai {
		display: grid;
		grid-template-columns: minmax(0, 1fr) auto;
		gap: 0.75rem;
		align-items: end;
		margin-top: 0.75rem;
	}
	.ai .field:nth-of-type(n + 2),
	.ai .btn.primary {
		grid-column: 1;
	}
	.ai .btn.primary {
		justify-self: start;
	}
	.auto {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 0.6rem 1rem;
		margin-top: 0.75rem;
	}
	.auto .check {
		margin: 0;
	}
	.age {
		display: inline-flex;
		align-items: center;
		gap: 0.5rem;
	}
	.age input {
		width: 5rem;
		padding: 0.4rem 0.6rem;
		border: 1px solid var(--line);
		border-radius: 9px;
		background: var(--surface);
	}
	.due {
		font-weight: 600;
	}
	.swipe-grid {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(14rem, 1fr));
		gap: 1rem;
		margin-top: 0.75rem;
	}
	fieldset {
		margin: 0;
		padding: 0.6rem 1rem 0.9rem;
		border: 1px solid var(--line);
		border-radius: var(--radius);
		background: var(--surface);
	}
	legend {
		padding: 0 0.4rem;
		font: 700 1rem var(--display);
	}
	fieldset .check {
		margin-top: 0.4rem;
	}
	.check {
		display: flex;
		gap: 0.5rem;
		margin-top: 0.9rem;
		font-size: 0.925rem;
	}
	.test {
		display: grid;
		gap: 0.2rem;
		margin-top: 1rem;
		padding: 0.7rem 0.9rem;
		background: var(--paper);
		border-radius: 9px;
	}
	.ok {
		color: var(--feed);
		font-weight: 600;
	}
	.actions {
		display: flex;
		flex-wrap: wrap;
		gap: 0.6rem;
		margin-top: 1.25rem;
	}
	.filter {
		display: flex;
		flex-wrap: wrap;
		gap: 0.4rem;
	}
	.filter .btn[aria-pressed='true'] {
		background: var(--ink);
		border-color: var(--ink);
		color: var(--paper);
	}
	.inline {
		display: flex;
		flex-wrap: wrap;
		align-items: end;
		gap: 0.75rem;
		margin-top: 0.75rem;
	}
	.inline .field {
		flex: 1;
		min-width: 12rem;
	}
</style>
