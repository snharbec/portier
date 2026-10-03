<script lang="ts">
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { api, type Category, type Thread } from '#lib/api.ts';
	import { app, categoryNames, classify, notify, refreshCounts, startDraft } from '#lib/app.svelte.ts';
	import ClassifyButtons from '#lib/components/ClassifyButtons.svelte';
	import MessageCard from '#lib/components/MessageCard.svelte';
	import DelayMenu from '#lib/components/DelayMenu.svelte';
	import { displayName, returnTime } from '#lib/format.ts';
	import { afterRemoving } from '#lib/reading.ts';
	import { rememberSearch, resultPath, search } from '#lib/search.svelte.ts';
	import { mailAction } from '#lib/swipe.ts';

	let thread = $state<Thread | null>(null);
	let error = $state('');
	let changing = $state(false);
	/** Messages that were unread when the thread was opened stay expanded. */
	let unreadAtOpen = $state(new Set<number>());

	const id = $derived(page.params.id);

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
					await api.post(`/threads/${requested}/seen`);
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
		const raw = page.url.searchParams.get('hit');
		const index = raw === null ? -1 : Number(raw);
		const valid = search.hits && Number.isInteger(index) && index >= 0 && index < search.hits.length;
		return valid && String(search.hits![index].thread_id) === id ? index : -1;
	});
	const hitMessage = $derived(hit >= 0 ? search.hits![hit].id : null);

	function toResult(index: number) {
		if (search.hits && index >= 0 && index < search.hits.length) goto(resultPath(index));
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
		if (hit >= 0) goto('/search');
		else if (history.length > 1) history.back();
		else goto('/');
	}

	let trashing = $state(false);

	const trash = () => putAway('trash', 'Moved to Trash: 1 conversation');
	const archive = () => putAway('archive', 'Archived: 1 conversation');
	/** Important and Inbox are two lists; the key i moves the conversation to the other one. */
	const toggleImportant = () =>
		thread?.important
			? putAway('unimportant', 'Moved to Inbox: 1 conversation')
			: putAway('important', 'Moved to Important: 1 conversation');
	const delay = (days: number) =>
		putAway('delay', `Delayed for ${days} ${days === 1 ? 'day' : 'days'}: 1 conversation`, { days });
	const undelay = () => putAway('undelay', 'Back in the Inbox: 1 conversation');
	let delayMenu: DelayMenu | undefined = $state();

	type PutAway = 'trash' | 'archive' | 'important' | 'unimportant' | 'delay' | 'undelay';

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
			if (onward) goto(onward, { replace: true });
			else if (history.length > 1) history.back();
			else goto('/');
		}
	}

	function onkeydown(event: KeyboardEvent) {
		const typing = (event.target as HTMLElement).closest('input, textarea, [contenteditable="true"]');
		if (typing || event.metaKey || event.ctrlKey || event.altKey) return;
		if (document.querySelector('dialog[open]')) return;
		if (event.key === 'r') draft('reply');
		else if (event.key === 'a') draft('reply_all');
		else if (event.key === 'f') draft('forward');
		else if (event.key === 'd' && thread?.can_trash) trash();
		else if (event.key === 'u') markUnread();
		else if (event.key === 'i' && thread?.can_archive) toggleImportant();
		else if (event.key === 'z' && thread?.can_archive && !thread.snoozed_until) {
			event.preventDefault();
			delayMenu?.show();
		}
		else if (event.key === 'e' && thread?.can_archive) archive();
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
			<button class="btn small" onclick={() => toResult(hit - 1)} disabled={hit === 0}>Previous</button>
			<button class="btn small" onclick={() => toResult(hit + 1)} disabled={hit === search.hits.length - 1}>
				Next
			</button>
		</nav>
	{/if}
	<div class="page-head stack reading">
		<h1>{thread.subject || '(no subject)'}</h1>
		{#if thread.snoozed_until}
			<p class="delayed">Delayed. Returns to the Inbox {returnTime(thread.snoozed_until)}.</p>
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
			</p>
			{#if changing}
				<ClassifyButtons small current={thread.sender.category} onpick={pick} />
			{/if}
		{/if}
		<div class="tools">
			<button class="btn primary" onclick={() => draft('reply')} title="Reply (r)">Reply</button>
			{#if last && last.to.length + last.cc.length > 1}
				<button class="btn" onclick={() => draft('reply_all')} title="Reply to all (a)">Reply all</button>
			{/if}
			<button class="btn" onclick={() => draft('forward')} title="Forward (f)">Forward</button>
			<span class="divider" aria-hidden="true"></span>
			{#if thread.can_archive}
				<button class="btn small" onclick={archive} disabled={trashing} title="Archive (e)">Archive</button>
				{#if thread.snoozed_until}
					<button class="btn small" onclick={undelay} disabled={trashing}>Back to Inbox now</button>
				{:else}
					<span title="Delay (z)"><DelayMenu bind:this={delayMenu} onpick={delay} disabled={trashing} /></span>
				{/if}
				<button class="btn small" onclick={toggleImportant} disabled={trashing} title="{thread.important ? 'Move to Inbox' : 'Move to Important'} (i)">
					{thread.important ? 'Move to Inbox' : 'Move to Important'}
				</button>
			{/if}
			{#if hasReceived}
				<button class="btn small" onclick={markUnread} disabled={trashing} title="Mark as unseen (u)">
					Mark as unseen
				</button>
			{/if}
			{#if thread.can_trash}
				<button class="btn small danger" onclick={trash} disabled={trashing} title="Move to Trash (d)">
					Move to Trash
				</button>
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
</style>
