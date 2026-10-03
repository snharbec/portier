<script lang="ts">
	import { page } from '$app/state';
	import { untrack } from 'svelte';
	import { api, type Message } from '#lib/api.ts';
	import { app } from '#lib/app.svelte.ts';
	import MessageCard from '#lib/components/MessageCard.svelte';
	import SelectAll from '#lib/components/SelectAll.svelte';
	import SelectionBar from '#lib/components/SelectionBar.svelte';
	import { createSelection } from '#lib/selection.svelte.ts';
	import { mailAction } from '#lib/swipe.ts';

	const PAGE = 20;
	const lists: Record<string, { title: string; back: string }> = {
		important: { title: 'Inbox', back: '/' },
		flagged: { title: 'Important', back: '/important' },
		feed: { title: 'Nice to know', back: '/feed' },
		junk: { title: 'Junk', back: '/junk' },
		sent: { title: 'Sent', back: '/sent' },
		archive: { title: 'Archive', back: '/archive' }
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
			const batch = await api.get<Message[]>(`/feed?box=${box}&offset=${offset}&limit=${limit}`);
			messages = more ? [...(messages ?? []), ...batch] : batch;
			done = batch.length < limit;
			error = '';
		} catch (e) {
			error = (e as Error).message;
		} finally {
			busy = false;
		}
	}

	let loadedBox = '';
	$effect(() => {
		app.tick;
		if (!list) return;
		if (loadedBox !== box) {
			loadedBox = box;
			messages = null;
			selection.clear();
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
				`Marked as read: ${count} mail${count === 1 ? '' : 's'}`
			);
		} catch (e) {
			error = (e as Error).message;
		}
	}
</script>

{#if !list}
	<p class="empty"><strong>There is no such list</strong><a class="btn" href="/">Go to Inbox</a></p>
{:else}
	<div class="page-head">
		<h1>{list.title}, all on one page</h1>
		<p>Every mail of this list, opened, newest first.</p>
		<p class="tools">
			<a class="btn small" href={list.back}>Show as list</a>
			{#if unread.length}
				<button class="btn small" onclick={markAllRead}>
					Mark {unread.length === 1 ? 'the unread mail' : `all ${unread.length} unread mails`} as read
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
			selected={picked.length}
			total={messages.length}
			onall={() => selection.set((messages ?? []).map((m) => m.id))}
			onnone={selection.clear}
		/>
		{#each messages as message (message.id)}
			<div class="row" class:unread={!message.seen && !message.is_outgoing}>
				<label class="pick">
					<input
						type="checkbox"
						checked={selection.has(message.id)}
						onchange={() => selection.toggle(message.id)}
						aria-label="Select {message.subject || '(no subject)'} from {message.from.name || message.from.address}"
					/>
				</label>
				<div class="card"><MessageCard {message} showSubject /></div>
			</div>
		{/each}
		{#if !done}
			<p class="more"><button class="btn" disabled={busy} onclick={() => load(true)}>Show older mail</button></p>
		{/if}
	{/if}

	<SelectionBar
		archivable={box !== 'archive'}
		list={box === 'flagged' ? 'important' : box === 'important' || box === 'feed' ? 'inbox' : 'other'}
		messageIds={picked.map((m) => m.id)}
		accountIds={[...new Set(picked.map((m) => m.account_id))]}
		total={messages?.length ?? 0}
		onselectall={() => selection.set((messages ?? []).map((m) => m.id))}
		onclear={selection.clear}
		ondone={selection.clear}
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
