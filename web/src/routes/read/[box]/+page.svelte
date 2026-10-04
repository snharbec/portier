<script lang="ts">
	import { page } from '$app/state';
	import { untrack } from 'svelte';
	import { api, type Message } from '#lib/api.ts';
	import { app } from '#lib/app.svelte.ts';
	import MessageCard from '#lib/components/MessageCard.svelte';
	import SelectAll from '#lib/components/SelectAll.svelte';
	import SelectionBar from '#lib/components/SelectionBar.svelte';
	import { search } from '#lib/search.svelte.ts';
	import { createSelection } from '#lib/selection.svelte.ts';
	import { mailAction } from '#lib/swipe.ts';

	const PAGE = 20;
	// Junk is left out on purpose: nobody should have every junk mail opened at once.
	const lists: Record<string, { title: string; back: string }> = {
		important: { title: 'Home', back: '/' },
		flagged: { title: 'Important', back: '/important' },
		delayed: { title: 'Delayed', back: '/delayed' },
		feed: { title: 'Nice to know', back: '/feed' },
		search: { title: 'Search results', back: '/search' },
		sent: { title: 'Sent', back: '/sent' },
		archive: { title: 'Archive', back: '/archive' },
		trash: { title: 'Trash', back: '/trash' }
	};

	const box = $derived(page.params.box ?? '');
	const list = $derived(lists[box]);

	let messages = $state<Message[] | null>(null);
	let done = $state(false);
	let busy = $state(false);
	let error = $state('');
	const selection = createSelection();
	const picked = $derived(selection.visible(messages ?? [], (m) => m.id));
	const unread = $derived((messages ?? []).filter((m) => !m.seen && !m.is_outgoing));

	/** `more` appends the next page; otherwise everything shown so far is fetched again. */
	async function load(more: boolean) {
		busy = true;
		try {
			const shown = messages?.length ?? 0;
			const [offset, limit] = more ? [shown, PAGE] : [0, Math.max(PAGE, shown)];
			const batch =
				box === 'search'
					? await searchBatch(offset, limit)
					: await api.get<Message[]>(`/feed?box=${box}&offset=${offset}&limit=${limit}`);
			messages = more ? [...(messages ?? []), ...batch] : batch;
			if (box !== 'search') done = batch.length < limit;
			error = '';
		} catch (e) {
			error = (e as Error).message;
		} finally {
			busy = false;
		}
	}

	/** The mails the last search found, in its order. One that is gone by now is left out. */
	async function searchBatch(offset: number, limit: number): Promise<Message[]> {
		const ids = (search.hits ?? []).slice(offset, offset + limit).map((hit) => hit.id);
		const loaded = await Promise.all(ids.map((id) => api.get<Message>(`/messages/${id}`).catch(() => null)));
		// A full batch must stay full for "is there more?"; the last one may be short anyway.
		done = offset + limit >= (search.hits?.length ?? 0);
		return loaded.filter((message): message is Message => message !== null);
	}

	let loadedBox = '';
	$effect(() => {
		app.tick;
		if (!list) return;
		if (loadedBox !== box) {
			loadedBox = box;
			messages = null;
			selection.stop();
		}
		// load() reads `messages`; untracked, so that storing the result does not restart this effect.
		untrack(() => load(false));
	});

	async function markAllRead() {
		const count = unread.length;
		try {
			await mailAction(
				'read',
				{ messageIds: unread.map((m) => m.id) },
				`Marked as seen: ${count} mail${count === 1 ? '' : 's'}`
			);
		} catch (e) {
			error = (e as Error).message;
		}
	}
</script>

{#if !list}
	<p class="empty"><strong>There is no such list</strong><a class="btn" href="/">Go to Home</a></p>
{:else}
	<div class="page-head reading">
		<h1>{list.title}, all on one page</h1>
		{#if box === 'search'}
			<p>Every mail found for “{search.answered}”, opened, newest first.</p>
		{:else}
			<p>Every mail of this list, opened, newest first.</p>
		{/if}
		<p class="tools">
			<a class="btn small" href={list.back}>Show as list</a>
			{#if unread.length}
				<button class="btn small" onclick={markAllRead}>
					Mark {unread.length === 1 ? 'the unseen mail' : `all ${unread.length} unseen mails`} as seen
				</button>
			{/if}
		</p>
	</div>

	{#if error}<p class="error" role="alert">{error}</p>{/if}

	{#if messages === null}
		<p class="empty" aria-busy="true">Loading</p>
	{:else if messages.length === 0}
		<div class="empty sheet">
			<strong>Nothing here</strong>
			This list has no mail at the moment.
		</div>
	{:else}
		<SelectAll
			{selection}
			selected={picked.length}
			total={messages.length}
			onall={() => selection.set((messages ?? []).map((m) => m.id))}
			onnone={selection.clear}
		/>
		{#each messages as message (message.id)}
			<div class="row" class:unread={!message.seen && !message.is_outgoing}>
				{#if selection.active}
				<label class="pick">
					<input
						type="checkbox"
						checked={selection.has(message.id)}
						onchange={() => selection.toggle(message.id)}
						aria-label="Select {message.subject || '(no subject)'} from {message.from.name || message.from.address}"
					/>
				</label>
				{/if}
				<div class="card"><MessageCard {message} showSubject /></div>
			</div>
		{/each}
		{#if !done}
			<p class="more"><button class="btn" disabled={busy} onclick={() => load(true)}>Show older mail</button></p>
		{/if}
	{/if}

	<SelectionBar
		archivable={box !== 'archive' && box !== 'trash'}
		trashable={box !== 'trash'}
		restorable={box === 'trash'}
		list={box === 'flagged' ? 'important' : box === 'delayed' ? 'delayed' : ['important', 'feed', 'search'].includes(box) ? 'inbox' : 'other'}
		messageIds={picked.map((m) => m.id)}
		accountIds={[...new Set(picked.map((m) => m.account_id))]}
		total={messages?.length ?? 0}
		onselectall={() => selection.set((messages ?? []).map((m) => m.id))}
		onclear={selection.clear}
		ondone={selection.stop}
	/>
{/if}

<style>
	.row {
		display: flex;
	}
	.row .pick {
		align-self: flex-start;
		height: 3.9rem;
	}
	.card {
		flex: 1;
		min-width: 0;
		border-left: 4px solid transparent;
		border-radius: var(--radius);
	}
	.unread .card {
		border-left-color: var(--important);
	}
	.more {
		text-align: center;
	}
</style>
