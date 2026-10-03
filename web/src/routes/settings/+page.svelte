<script lang="ts">
	import { api, type Account, type Category, type Sender, type User } from '#lib/api.ts';
	import { app, categoryNames, classify, refreshAccounts } from '#lib/app.svelte.ts';
	import ClassifyButtons from '#lib/components/ClassifyButtons.svelte';
	import { displayName, fullDate } from '#lib/format.ts';

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
			}>('/accounts/test', form);
			testResult = result;
			form.junk_folder ||= result.junk_folder ?? '';
			form.sent_folder ||= result.sent_folder ?? '';
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

<div class="page-head"><h1>Settings</h1></div>

<section>
	<h2>Mail accounts</h2>
	{#if app.accounts.length === 0 && !form}
		<p class="muted">No account connected yet. emscreen works with any mailbox that offers IMAP and SMTP.</p>
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
						<span class="small">Remove this account and its mail from emscreen? The mailbox itself is untouched.</span>
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

			<h4>Folders</h4>
			<div class="grid">
				<label class="field">Inbox <input bind:value={form.inbox_folder} /></label>
				<label class="field">Junk <input bind:value={form.junk_folder} placeholder="Found automatically" /></label>
				<label class="field">Sent <input bind:value={form.sent_folder} placeholder="Found automatically" /></label>
			</div>
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

<section>
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
				<li>
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
				</li>
			{/each}
		</ul>
	{/if}
</section>

{#if app.user?.is_admin}
	<section>
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

<section>
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
	section {
		margin-bottom: 3rem;
	}
	section > h2 {
		margin-bottom: 0.6rem;
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
