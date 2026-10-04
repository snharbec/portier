<script lang="ts">
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { api, type Category, type Thread } from '#lib/api.ts';
	import { app, categoryNames, classify, notify, refreshCounts, startDraft } from '#lib/app.svelte.ts';
	import ClassifyButtons from './ClassifyButtons.svelte';
	import MessageCard from './MessageCard.svelte';
	import SenderPicture from './SenderPicture.svelte';
	import DelayMenu from './DelayMenu.svelte';
	import { displayName, returnTime } from '#lib/format.ts';
	import { afterRemoving, listPath, neighbour } from '#lib/reading.ts';
	import { rememberSearch, resultPath, runSearch, search } from '#lib/search.svelte.ts';
	import { mailAction } from '#lib/swipe.ts';

	let thread = $state<Thread | null>(null);
	let error = $state('');
	let changing = $state(false);
	let choosingPicture = $state(false);
	/** Messages that were unread when the thread was opened stay expanded. */
	let unreadAtOpen = $state(new Set<number>());

	let {
		id,
		embedded = false
	}: {
		/** The conversation to show. */
		id: string;
		/** Shown beside a mail list (split view) instead of as a page of its own. */
		embedded?: boolean;
	} = $props();

	// Beside a list, "going somewhere" means changing what the pane shows, on the list's own address.
	const closePane = () => goto(page.url.pathname, { replace: true, reset: false });
	const openInPane = (threadId: string) => goto(`${page.url.pathname}?open=${threadId}`, { replace: true, reset: false });

	/** Set once this conversation was moved to Trash: it no longer exists, so stop reloading it. */
	let leaving = false;

	async function load(firstLoad: boolean) {
		const requested = id;
		// An answer for a conversation the reader has already left must not touch the page.
		const current = () => requested === id && !leaving;
		try {
			const loaded = await api.get<Thread>(`/threads/${requested}`);
			if (!current()) return;
			if (firstLoad) {
				unreadAtOpen = new Set(loaded.messages.filter((m) => !m.seen).map((m) => m.id));
				if (unreadAtOpen.size) {
					// Without the server (mail kept on this device) the mail is read all the same;
					// it stays unseen there.
					await api.post(`/threads/${requested}/seen`).catch(() => {});
					if (!current()) return;
					refreshCounts().catch(() => {});
				}
			}
			thread = loaded;
			error = '';
		} catch (e) {
			if (current()) error = (e as Error).message;
		}
	}

	let loadedId = '';
	$effect(() => {
		app.tick;
		const first = loadedId !== id;
		loadedId = id ?? '';
		if (first) leaving = false;
		if (!leaving) load(first);
	});

	async function pick(category: Category) {
		if (!thread?.sender) return;
		await classify(thread.sender.id, category);
		changing = false;
	}

	const last = $derived(thread?.messages.at(-1));

	/** Position in the last search's results, when this conversation was opened from there. */
	const hit = $derived.by(() => {
		const raw = embedded ? null : page.url.searchParams.get('hit');
		const index = raw === null ? -1 : Number(raw);
		const valid = search.hits && Number.isInteger(index) && index >= 0 && index < search.hits.length;
		return valid && String(search.hits![index].thread_id) === id ? index : -1;
	});
	const hitMessage = $derived(hit >= 0 ? search.hits![hit].id : null);

	function toResult(index: number) {
		if (search.hits && index >= 0 && index < search.hits.length) goto(resultPath(index));
	}

	/** The mail before or after this one: in the search results, or in the list it was opened from. */
	function toNeighbour(step: -1 | 1) {
		if (hit >= 0) return toResult(hit + step);
		const next = neighbour(Number(id), step);
		if (next !== undefined) goto(`/thread/${next}`, { replace: true });
	}

	// ---- Your own note on the conversation ----
	/** The text being edited, or null while the note is only shown. */
	let noteDraft = $state<string | null>(null);
	let noteField: HTMLTextAreaElement | undefined = $state();

	function editNote() {
		noteDraft = thread?.note ?? '';
		queueMicrotask(() => noteField?.focus());
	}

	async function saveNote() {
		if (!thread || noteDraft === null) return;
		try {
			const saved = await api.put<{ note: string; undo?: string }>(`/threads/${thread.id}/note`, {
				note: noteDraft
			});
			thread.note = saved.note;
			noteDraft = null;
			notify(saved.note ? 'Note saved' : 'Note removed', saved.undo);
			app.tick += 1;
		} catch (e) {
			notify((e as Error).message);
		}
	}

	function noteKeydown(event: KeyboardEvent) {
		if (event.key === 'Enter' && (event.ctrlKey || event.metaKey)) {
			event.preventDefault();
			saveNote();
		} else if (event.key === 'Escape') {
			event.preventDefault();
			noteDraft = null;
		}
	}

	async function draft(kind: string) {
		if (last) goto(await startDraft(kind, last.id));
	}

	/** Only received mail has a read state. */
	const hasReceived = $derived(thread?.messages.some((m) => !m.is_outgoing) ?? false);

	/** Marks the conversation unread again and returns to the list, where it shows as new. */
	async function markUnread() {
		if (!thread || !hasReceived || trashing) return;
		try {
			await mailAction('unread', { threadIds: [thread.id] }, 'Marked as unseen: 1 conversation');
		} catch (e) {
			notify((e as Error).message);
			return;
		}
		if (embedded) closePane();
		else if (hit >= 0) goto('/search');
		else goto(listPath(), { replace: true });
	}

	let trashing = $state(false);

	const trash = () => putAway('trash', 'Moved to Trash: 1 conversation');
	const archive = () => putAway('archive', 'Archived: 1 conversation');
	/** Important and Home are two lists; the key i moves the conversation to the other one. */
	const toggleImportant = () =>
		thread?.important
			? putAway('unimportant', 'Moved to Home: 1 conversation')
			: putAway('important', 'Moved to Important: 1 conversation');
	const delay = (days: number) =>
		putAway('delay', `Delayed for ${days} ${days === 1 ? 'day' : 'days'}: 1 conversation`, { days });
	const undelay = () => putAway('undelay', 'Back in Home: 1 conversation');
	const untrash = () => putAway('untrash', 'Moved back to Home: 1 conversation');
	let delayMenu: DelayMenu | undefined = $state();

	type PutAway = 'trash' | 'untrash' | 'archive' | 'important' | 'unimportant' | 'delay' | 'undelay';

	/** Takes the whole conversation out of the list it is in, then goes on to where the reader came from. */
	async function putAway(action: PutAway, done: string, extra: Record<string, unknown> = {}) {
		if (!thread || trashing) return;
		trashing = true;
		const threadId = thread.id;
		const position = hit;
		try {
			leaving = true;
			await mailAction(action, { threadIds: [threadId] }, done, extra);
		} catch (e) {
			leaving = false;
			notify((e as Error).message);
			trashing = false;
			return;
		}
		trashing = false;
		const gone = action === 'trash';
		if (position >= 0 && search.hits && !gone) {
			// Still findable: stay in the results and move on to the next one.
			goto(position + 1 < search.hits.length ? resultPath(position + 1) : '/search', { replace: true });
		} else if (position >= 0 && search.hits) {
			// Opened from a search: drop its results and show the one that takes its place.
			search.hits = search.hits.filter((h) => h.thread_id !== threadId);
			rememberSearch();
			const next = Math.min(position, search.hits.length - 1);
			goto(next >= 0 ? resultPath(next) : '/search', { replace: true });
		} else {
			// Opened from a list: go straight on to the conversation that followed this one.
			const onward = afterRemoving(threadId);
			if (embedded) {
				if (onward?.startsWith('/thread/')) openInPane(onward.slice('/thread/'.length));
				else closePane();
				// Beside search results: the list is a search, which has to be asked again.
				if (page.url.pathname === '/search') runSearch();
			} else goto(onward ?? listPath(), { replace: true });
		}
	}

	function onkeydown(event: KeyboardEvent) {
		const typing = (event.target as HTMLElement).closest('input, textarea, [contenteditable="true"]');
		if (typing || event.metaKey || event.ctrlKey || event.altKey) return;
		if (document.querySelector('dialog[open]')) return;
		if (event.key === 'r') draft('reply');
		else if (event.key === 'R') draft('reply_all');
		else if (event.key === 'f') draft('forward');
		else if (event.key === 'd' && thread?.can_trash) trash();
		else if (event.key === 'u') markUnread();
		else if (event.key === 'i' && thread?.can_archive) toggleImportant();
		else if (event.key === 'z' && thread?.can_archive && !thread.snoozed_until) {
			event.preventDefault();
			delayMenu?.show();
		}
		else if (event.key === 'a' && thread?.can_archive) archive();
		// Beside a list the arrows step through the list's rows; the layout does that.
		else if (!embedded && (event.key === 'ArrowDown' || event.key === 'ArrowUp')) {
			event.preventDefault();
			toNeighbour(event.key === 'ArrowDown' ? 1 : -1);
		}
		else if (event.key === 't') {
			event.preventDefault();
			editNote();
		}
		else if (hit >= 0 && event.key === 'p') toResult(hit - 1);
		else if (hit >= 0 && event.key === 'n') toResult(hit + 1);
	}
</script>

<svelte:window {onkeydown} />

{#if error}
	<p class="empty"><strong>This conversation cannot be shown</strong>{error}</p>
{:else if !thread}
	<p class="empty" aria-busy="true">Loading</p>
{:else}
	{#if hit >= 0 && search.hits}
		<nav class="results" aria-label="Search results">
			<a class="btn small" href="/search">Back to results</a>
			<span class="muted">Result {hit + 1} of {search.hits.length} for “{search.answered}”</span>
			<button class="btn small" onclick={() => toResult(hit - 1)} disabled={hit === 0}>Previous <span class="key">(p)</span></button>
			<button class="btn small" onclick={() => toResult(hit + 1)} disabled={hit === search.hits.length - 1}>
				Next <span class="key">(n)</span>
			</button>
		</nav>
	{/if}
	<div class="page-head stack reading">
		<h1>{thread.subject || '(no subject)'}</h1>
		{#if thread.snoozed_until}
			<p class="delayed">Delayed. Returns to Home {returnTime(thread.snoozed_until)}.</p>
		{/if}
		{#if thread.sender}
			<p class="sender">
				<span title={thread.sender.address}>{displayName(thread.sender.display_name, thread.sender.address)}</span>
				{#if thread.sender.category}
					<span class="tag {thread.sender.category}">{categoryNames[thread.sender.category]}</span>
				{:else}
					<span class="tag">Not screened yet</span>
				{/if}
				<button class="btn small quiet" onclick={() => (changing = !changing)}>
					{thread.sender.category ? 'Change' : 'Decide'}
				</button>
				<button class="btn small quiet" aria-expanded={choosingPicture} onclick={() => (choosingPicture = !choosingPicture)}>
					Picture
				</button>
			</p>
			{#if choosingPicture}
				<SenderPicture
					senderId={thread.sender.id}
					address={thread.sender.address}
					name={displayName(thread.sender.display_name, thread.sender.address)}
					hasPicture={thread.sender.has_picture}
					onchange={(has) => {
						if (thread?.sender) thread.sender.has_picture = has;
					}}
				/>
			{/if}
			{#if changing}
				<ClassifyButtons small current={thread.sender.category} onpick={pick} />
			{/if}
		{/if}
		{#if noteDraft !== null}
			<div class="note editing">
				<label for="note-{thread.id}">Your note on this conversation</label>
				<textarea
					id="note-{thread.id}"
					bind:this={noteField}
					bind:value={noteDraft}
					onkeydown={noteKeydown}
					rows="3"
					maxlength="2000"
					placeholder="What you want to remember or find again, e.g. tax 2026, warranty until May"
				></textarea>
				<div class="note-tools">
					<button class="btn small primary" onclick={saveNote}>Save note <span class="key">(Ctrl Return)</span></button>
					<button class="btn small quiet" onclick={() => (noteDraft = null)}>Cancel</button>
					<span class="muted">Only you see it. The search finds its words.</span>
				</div>
			</div>
		{:else if thread.note}
			<div class="note">
				<p>{thread.note}</p>
				<button class="btn small quiet" onclick={editNote}>Edit note <span class="key">(t)</span></button>
			</div>
		{/if}
		<div class="tools">
			<button class="btn primary" onclick={() => draft('reply')}>Reply <span class="key">(r)</span></button>
			{#if last && last.to.length + last.cc.length > 1}
				<button class="btn" onclick={() => draft('reply_all')}>Reply all <span class="key">(Shift r)</span></button>
			{/if}
			<button class="btn" onclick={() => draft('forward')}>Forward <span class="key">(f)</span></button>
			<span class="divider" aria-hidden="true"></span>
			{#if thread.can_archive}
				<button class="btn small" onclick={archive} disabled={trashing}>Archive <span class="key">(a)</span></button>
				{#if thread.snoozed_until}
					<button class="btn small" onclick={undelay} disabled={trashing}>Back to Home now</button>
				{:else}
					<DelayMenu bind:this={delayMenu} onpick={delay} disabled={trashing} key="z" />
				{/if}
				<button class="btn small" onclick={toggleImportant} disabled={trashing}>
					{thread.important ? 'Move to Home' : 'Move to Important'} <span class="key">(i)</span>
				</button>
			{/if}
			{#if hasReceived}
				<button class="btn small" onclick={markUnread} disabled={trashing}>
					Mark as unseen <span class="key">(u)</span>
				</button>
			{/if}
			{#if !thread.note && noteDraft === null}
				<button class="btn small" onclick={editNote}>Add note <span class="key">(t)</span></button>
			{/if}
			<a
				class="btn small"
				href={thread.messages.length === 1
					? `/api/export/mail?message=${thread.messages[0].id}`
					: `/api/export/mail?threads=${thread.id}`}
				download
				title={thread.messages.length === 1
					? 'Download this mail as an .eml file'
					: 'Download this conversation as an mbox file'}
			>
				Save as file
			</a>
			{#if thread.can_restore}
				<button class="btn small" onclick={untrash} disabled={trashing} title="Take it out of the Trash">
					Move back to Home
				</button>
			{/if}
			{#if thread.can_trash}
				<button class="btn small danger" onclick={trash} disabled={trashing}>
					Move to Trash <span class="key">(d)</span>
				</button>
			{/if}
			{#if embedded}
				<a class="btn small quiet" href="/thread/{thread.id}" title="Show this conversation on a page of its own">
					Open full page
				</a>
				<button class="btn small quiet" onclick={closePane}>Close</button>
			{/if}
		</div>
	</div>

	{#each thread.messages as message, index (message.id)}
		<MessageCard
			{message}
			open={index === thread.messages.length - 1 || unreadAtOpen.has(message.id) || message.id === hitMessage}
			actions={thread.messages.length > 1}
		/>
	{/each}

{/if}

<style>
	.results {
		display: flex;
		align-items: center;
		flex-wrap: wrap;
		gap: 0.5rem;
		margin-top: 1.25rem;
		padding: 0.5rem 0.6rem;
		background: var(--surface);
		border: 1px solid var(--line);
		border-radius: 999px;
	}
	.results span {
		flex: 1;
		min-width: 8rem;
		font-size: 0.9rem;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.sender {
		display: flex;
		align-items: center;
		flex-wrap: wrap;
		gap: 0.6rem;
		margin-bottom: 0.6rem;
	}
	.tools {
		margin: 0.9rem 0 0;
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 0.5rem;
	}
	.divider {
		width: 1px;
		align-self: stretch;
		margin-inline: 0.35rem;
		background: var(--line);
	}
	.page-head .delayed {
		color: var(--feed);
		font-weight: 600;
	}
	/* Your own words, set apart from the mail by the signal colour of a sticky note. */
	.note {
		display: flex;
		align-items: flex-start;
		gap: 0.75rem;
		margin: 0.6rem 0 0.2rem;
		padding: 0.55rem 0.8rem;
		border-left: 4px solid var(--signal);
		border-radius: 0 9px 9px 0;
		background: color-mix(in srgb, var(--signal) 16%, var(--surface));
		max-width: 60rem;
	}
	.note p {
		flex: 1;
		margin: 0;
		color: var(--ink);
		white-space: pre-wrap;
		overflow-wrap: anywhere;
		max-width: none;
	}
	.note.editing {
		display: grid;
		gap: 0.4rem;
	}
	.note label {
		font-weight: 600;
		font-size: 0.9rem;
	}
	.note textarea {
		width: 100%;
		padding: 0.5rem 0.65rem;
		border: 1px solid var(--line);
		border-radius: 9px;
		background: var(--surface);
		resize: vertical;
	}
	.note-tools {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 0.5rem;
		font-size: 0.875rem;
	}
</style>
