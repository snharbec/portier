<script lang="ts">
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { api, type Addr, type DraftAttachment, type DraftDetail, type Message } from '#lib/api.ts';
	import { app, draftReturnPath, refreshCounts } from '#lib/app.svelte.ts';
	import Editor from '#lib/components/Editor.svelte';
	import MessageCard from '#lib/components/MessageCard.svelte';
	import { displayName, fileSize } from '#lib/format.ts';
	import { onMount } from 'svelte';

	const id = page.params.id;

	let detail = $state<DraftDetail | null>(null);
	/** The mail being answered or forwarded, shown below the draft to read while writing. */
	let original = $state<Message | null>(null);
	let attachments = $state<DraftAttachment[]>([]);
	let error = $state('');
	let status = $state('');
	let sending = $state(false);
	let showCopies = $state(false);
	let suggestions = $state<Addr[]>([]);
	/** Suggestion picked with the arrow keys; -1 while none is. */
	let chosen = $state(-1);
	let toField: HTMLInputElement | undefined = $state();
	let saveTimer: ReturnType<typeof setTimeout> | undefined;
	let dirty = false;

	onMount(() => {
		api.get<DraftDetail>(`/drafts/${id}`)
			.then((loaded) => {
				detail = loaded;
				attachments = loaded.attachments;
				showCopies = !!(loaded.draft.cc_addrs || loaded.draft.bcc_addrs);
				if (loaded.source) {
					// Only for reading along: without it the draft works all the same.
					api.get<Message>(`/messages/${loaded.source.id}`)
						.then((message) => (original = message))
						.catch(() => {});
				}
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
		chosen = -1;
	}

	/** Arrow keys walk through the suggestions, Enter or Tab takes the picked one, Escape closes them. */
	function toKeydown(event: KeyboardEvent) {
		if (!suggestions.length || event.ctrlKey || event.metaKey || event.altKey) return;
		if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
			event.preventDefault();
			const step = event.key === 'ArrowDown' ? 1 : -1;
			// From the field itself Down picks the first and Up the last; past either end it wraps.
			chosen = chosen < 0 && step < 0 ? suggestions.length - 1 : (chosen + step + suggestions.length) % suggestions.length;
			document.getElementById(`to-suggestion-${chosen}`)?.scrollIntoView({ block: 'nearest' });
		} else if ((event.key === 'Enter' || event.key === 'Tab') && chosen >= 0) {
			event.preventDefault();
			accept(suggestions[chosen]);
		} else if (event.key === 'Escape') {
			event.preventDefault();
			suggestions = [];
			chosen = -1;
		}
	}

	function accept(contact: Addr) {
		if (!detail) return;
		const parts = detail.draft.to_addrs.split(',').slice(0, -1).map((p) => p.trim());
		detail.draft.to_addrs = [...parts, contact.address].join(', ') + ', ';
		suggestions = [];
		chosen = -1;
		changed();
		// Ready for the next recipient, also after a click on a suggestion.
		toField?.focus();
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

	/** Ctrl+Return (or Cmd+Return) sends from anywhere in the composer, also from inside the text. */
	function onkeydown(event: KeyboardEvent) {
		if (event.key !== 'Enter' || !(event.ctrlKey || event.metaKey) || event.altKey || event.shiftKey) return;
		if (document.querySelector('dialog[open]')) return;
		// Before the editor sees it, which would add a line break.
		event.preventDefault();
		event.stopPropagation();
		if (app.accounts.length > 0) send();
	}

	async function send() {
		if (!detail || sending) return;
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
		goto(draftReturnPath(id), { replace: true });
	}
</script>

<svelte:window onkeydowncapture={onkeydown} />

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
			<!-- The browser's own suggestions (contacts, autofill) would compete with the list below:
			     nothing here may look like a name or address field to it, placeholder included. -->
			<input
				id="to"
				type="text"
				name="recipients-{id}"
				role="combobox"
				aria-autocomplete="list"
				aria-expanded={suggestions.length > 0}
				aria-controls="to-suggestions"
				aria-activedescendant={chosen >= 0 ? `to-suggestion-${chosen}` : undefined}
				bind:this={toField}
				onkeydown={toKeydown}
				bind:value={detail.draft.to_addrs}
				oninput={() => {
					changed();
					suggest();
				}}
				autocomplete="off"
				autocorrect="off"
				autocapitalize="off"
				spellcheck="false"
				data-1p-ignore
				data-lpignore="true"
				data-form-type="other"
				placeholder="Who is it for? Separate several with commas"
			/>
			{#if !showCopies}
				<button class="btn small quiet" onclick={() => (showCopies = true)}>Cc, Bcc</button>
			{/if}
			{#if suggestions.length}
				<ul class="suggestions sheet" id="to-suggestions" role="listbox" aria-label="Suggested recipients">
					{#each suggestions as contact, index}
						<li role="presentation">
							<button
								id="to-suggestion-{index}"
								role="option"
								aria-selected={index === chosen}
								tabindex="-1"
								onclick={() => accept(contact)}
							>
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
				<input type="text" name="copies-{id}" bind:value={detail.draft.cc_addrs} oninput={changed} autocomplete="off" autocorrect="off" autocapitalize="off" spellcheck="false" data-1p-ignore data-lpignore="true" data-form-type="other" />
			</label>
			<label class="row">
				<span>Bcc</span>
				<input type="text" name="blind-copies-{id}" bind:value={detail.draft.bcc_addrs} oninput={changed} autocomplete="off" autocorrect="off" autocapitalize="off" spellcheck="false" data-1p-ignore data-lpignore="true" data-form-type="other" />
			</label>
		{/if}
		<label class="row">
			<span>Subject</span>
			<input class="subject" bind:value={detail.draft.subject} oninput={changed} />
		</label>

		<!-- A reply has its recipients and subject already: writing starts in the text. -->
		<Editor
			bind:value={detail.draft.body_html}
			onchange={changed}
			autofocus={detail.draft.kind === 'reply' || detail.draft.kind === 'reply_all'}
		/>

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
			<button class="btn primary" onclick={send} disabled={sending || app.accounts.length === 0} title="Send (Ctrl+Return)">
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
	{#if original}
		<section class="original" aria-label="Original message">
			<h2>{detail.draft.kind === 'forward' ? 'The mail you forward' : 'The mail you answer'}</h2>
			<MessageCard message={original} actions={false} showSubject />
		</section>
	{/if}
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
	.suggestions button[aria-selected='true'] {
		background: color-mix(in srgb, var(--important) 14%, var(--surface));
	}
	.original {
		margin-top: 1.5rem;
	}
	.original h2 {
		font: 600 0.9rem var(--body);
		letter-spacing: 0;
		color: var(--ink-soft);
		margin-bottom: 0.5rem;
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
