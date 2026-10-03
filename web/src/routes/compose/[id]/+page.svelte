<script lang="ts">
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { api, type Addr, type DraftAttachment, type DraftDetail } from '#lib/api.ts';
	import { app, refreshCounts } from '#lib/app.svelte.ts';
	import Editor from '#lib/components/Editor.svelte';
	import { displayName, fileSize } from '#lib/format.ts';
	import { onMount } from 'svelte';

	const id = page.params.id;

	let detail = $state<DraftDetail | null>(null);
	let attachments = $state<DraftAttachment[]>([]);
	let error = $state('');
	let status = $state('');
	let sending = $state(false);
	let showCopies = $state(false);
	let suggestions = $state<Addr[]>([]);
	let saveTimer: ReturnType<typeof setTimeout> | undefined;
	let dirty = false;

	onMount(() => {
		api.get<DraftDetail>(`/drafts/${id}`)
			.then((loaded) => {
				detail = loaded;
				attachments = loaded.attachments;
				showCopies = !!(loaded.draft.cc_addrs || loaded.draft.bcc_addrs);
			})
			.catch((e) => (error = e.message));
		return () => {
			clearTimeout(saveTimer);
			if (dirty) save().catch(() => {});
		};
	});

	const titles = { new: 'New message', reply: 'Reply', reply_all: 'Reply to all', forward: 'Forward' };

	async function save() {
		if (!detail) return;
		dirty = false;
		await api.put(`/drafts/${id}`, detail.draft);
		status = 'Draft saved';
	}

	function changed() {
		dirty = true;
		status = '';
		clearTimeout(saveTimer);
		saveTimer = setTimeout(() => save().catch((e) => (error = e.message)), 800);
	}

	async function suggest() {
		if (!detail) return;
		const term = detail.draft.to_addrs.split(',').at(-1)?.trim() ?? '';
		suggestions = term.length >= 2 ? await api.get<Addr[]>(`/contacts?q=${encodeURIComponent(term)}`) : [];
	}

	function accept(contact: Addr) {
		if (!detail) return;
		const parts = detail.draft.to_addrs.split(',').slice(0, -1).map((p) => p.trim());
		detail.draft.to_addrs = [...parts, contact.address].join(', ') + ', ';
		suggestions = [];
		changed();
	}

	async function upload(event: Event) {
		const input = event.target as HTMLInputElement;
		if (!input.files?.length) return;
		const form = new FormData();
		for (const file of input.files) form.append('file', file);
		try {
			attachments = await api.post<DraftAttachment[]>(`/drafts/${id}/attachments`, form);
		} catch (e) {
			error = (e as Error).message;
		}
		input.value = '';
	}

	async function removeFile(attachmentId: number) {
		attachments = await api.delete<DraftAttachment[]>(`/drafts/${id}/attachments/${attachmentId}`);
	}

	async function send() {
		if (!detail) return;
		sending = true;
		error = '';
		try {
			clearTimeout(saveTimer);
			await save();
			await api.post(`/drafts/${id}/send`);
			dirty = false;
			await refreshCounts();
			leave();
		} catch (e) {
			error = (e as Error).message;
		} finally {
			sending = false;
		}
	}

	async function discard() {
		clearTimeout(saveTimer);
		dirty = false;
		await api.delete(`/drafts/${id}`);
		await refreshCounts();
		leave();
	}

	/** Back to where the draft was started: the conversation for a reply, otherwise the list. */
	function leave() {
		if (history.length > 1) history.back();
		else goto('/');
	}
</script>

{#if !detail}
	{#if error}
		<p class="empty"><strong>This draft cannot be opened</strong>{error}</p>
	{:else}
		<p class="empty" aria-busy="true">Loading</p>
	{/if}
{:else}
	<div class="page-head measure">
		<h1>{titles[detail.draft.kind]}</h1>
	</div>

	<div class="sheet composer">
		{#if app.accounts.length === 0}
			<p class="error">Add a mail account in Settings before you can send.</p>
		{:else}
			<label class="row">
				<span>From</span>
				<select bind:value={detail.draft.account_id} onchange={changed}>
					{#each app.accounts as account}
						<option value={account.id}>{account.label} ({account.address})</option>
					{/each}
				</select>
			</label>
		{/if}
		<div class="row to">
			<label for="to">To</label>
			<input
				id="to"
				bind:value={detail.draft.to_addrs}
				oninput={() => {
					changed();
					suggest();
				}}
				autocomplete="off"
				placeholder="name@example.com, another@example.com"
			/>
			{#if !showCopies}
				<button class="btn small quiet" onclick={() => (showCopies = true)}>Cc, Bcc</button>
			{/if}
			{#if suggestions.length}
				<ul class="suggestions sheet">
					{#each suggestions as contact}
						<li>
							<button onclick={() => accept(contact)}>
								<strong>{displayName(contact.name, contact.address)}</strong>
								<span class="muted">{contact.address}</span>
							</button>
						</li>
					{/each}
				</ul>
			{/if}
		</div>
		{#if showCopies}
			<label class="row">
				<span>Cc</span>
				<input bind:value={detail.draft.cc_addrs} oninput={changed} autocomplete="off" />
			</label>
			<label class="row">
				<span>Bcc</span>
				<input bind:value={detail.draft.bcc_addrs} oninput={changed} autocomplete="off" />
			</label>
		{/if}
		<label class="row">
			<span>Subject</span>
			<input class="subject" bind:value={detail.draft.subject} oninput={changed} />
		</label>

		<Editor bind:value={detail.draft.body_html} onchange={changed} />

		{#if detail.source}
			<div class="source">
				<label>
					<input type="checkbox" bind:checked={detail.draft.include_quote} onchange={changed} />
					Include the message from {displayName(detail.source.from.name, detail.source.from.address)} below yours
				</label>
				{#if detail.draft.kind === 'forward' && detail.source.attachments > 0}
					<label>
						<input type="checkbox" bind:checked={detail.draft.forward_attachments} onchange={changed} />
						Forward its {detail.source.attachments === 1 ? 'attachment' : `${detail.source.attachments} attachments`}
					</label>
				{/if}
			</div>
		{/if}

		{#if attachments.length}
			<ul class="files">
				{#each attachments as file (file.id)}
					<li>
						{file.filename} <span class="muted">{fileSize(file.size)}</span>
						<button class="btn small quiet" onclick={() => removeFile(file.id)} aria-label="Remove {file.filename}">
							Remove
						</button>
					</li>
				{/each}
			</ul>
		{/if}

		{#if error}<p class="error" role="alert">{error}</p>{/if}

		<div class="actions">
			<button class="btn primary" onclick={send} disabled={sending || app.accounts.length === 0}>
				{sending ? 'Sending' : 'Send'}
			</button>
			<label class="btn">
				Attach files
				<input type="file" multiple onchange={upload} hidden />
			</label>
			<span class="muted status" aria-live="polite">{status}</span>
			<button class="btn quiet danger" onclick={discard}>Discard</button>
		</div>
	</div>
{/if}

<style>
	/* Forms stay at a readable width instead of stretching across the window. */
	:global(main:has(> .measure)) {
		max-width: var(--column);
	}
	.composer {
		padding: 0.5rem 1.2rem 1.2rem;
		overflow: visible;
	}
	.row {
		display: flex;
		align-items: center;
		gap: 0.75rem;
		border-bottom: 1px solid var(--line);
		position: relative;
	}
	.row > span,
	.row > label {
		flex: none;
		width: 4rem;
		color: var(--ink-soft);
		font-size: 0.9rem;
	}
	.row input,
	.row select {
		flex: 1;
		min-width: 0;
		border: 0;
		background: none;
		padding: 0.7rem 0;
		outline-offset: -2px;
	}
	.subject {
		font-weight: 700;
	}
	.suggestions {
		position: absolute;
		top: 100%;
		left: 4.75rem;
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
	.suggestions button:hover,
	.suggestions button:focus-visible {
		background: var(--paper);
	}
	.source {
		display: grid;
		gap: 0.35rem;
		padding: 0.8rem 0;
		border-top: 1px solid var(--line);
		font-size: 0.925rem;
	}
	.files {
		list-style: none;
		padding: 0;
		margin: 0 0 0.75rem;
		display: grid;
		gap: 0.25rem;
	}
	.actions {
		display: flex;
		align-items: center;
		gap: 0.6rem;
		flex-wrap: wrap;
		padding-top: 0.9rem;
		border-top: 1px solid var(--line);
	}
	.status {
		font-size: 0.875rem;
	}
	.actions .danger {
		margin-left: auto;
	}
</style>
