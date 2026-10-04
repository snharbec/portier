<script lang="ts">
	import { api, type ThreadSummary } from '#lib/api.ts';
	import { page } from '$app/state';
	import { app } from '#lib/app.svelte.ts';
	import SelectAll from '#lib/components/SelectAll.svelte';
	import SelectionBar from '#lib/components/SelectionBar.svelte';
	import ThreadList from '#lib/components/ThreadList.svelte';
	import { createSelection } from '#lib/selection.svelte.ts';

	let threads = $state<ThreadSummary[] | null>(null);
	let error = $state('');
	const selection = createSelection();

	// "Unseen only" narrows the list to conversations with unseen mail; kept in this browser.
	const UNSEEN_KEY = 'emscreen.unseen-only.archive';
	let unseenOnly = $state(restoreUnseenOnly());

	function restoreUnseenOnly() {
		try {
			return localStorage.getItem(UNSEEN_KEY) === '1';
		} catch {
			return false;
		}
	}

	function toggleUnseenOnly() {
		unseenOnly = !unseenOnly;
		selection.stop();
		try {
			localStorage.setItem(UNSEEN_KEY, unseenOnly ? '1' : '0');
		} catch {
			// The choice then lasts until the page is reloaded.
		}
	}

	// The mail open beside the list stays in it, although opening it made it seen.
	const shown = $derived(
		unseenOnly
			? (threads ?? []).filter((t) => t.unread > 0 || String(t.id) === page.url.searchParams.get('open'))
			: (threads ?? [])
	);
	const picked = $derived(selection.visible(shown, (t) => t.id));

	$effect(() => {
		app.tick;
		api.get<ThreadSummary[]>('/threads?box=archive')
			.then((list) => (threads = list))
			.catch((e) => (error = e.message));
	});
</script>

<div class="page-head">
	<h1>Archive</h1>
	<p>Mail you archived. It sits in the Archive folder of your mail account.</p>
	{#if threads?.length}
		<p class="tools">
			<button class="btn small" aria-pressed={unseenOnly} onclick={toggleUnseenOnly}>Unseen only</button>
			<a class="btn small" href="/read/archive">Read all on one page</a>
		</p>
	{/if}
</div>

{#if error}
	<p class="error" role="alert">{error}</p>
{:else if threads === null}
	<p class="empty" aria-busy="true">Loading</p>
{:else if threads.length === 0}
	<div class="empty sheet">
		<strong>Nothing archived yet</strong>
		Archive a mail from any list or from the mail itself and it moves here.
	</div>
{:else if shown.length === 0}
	<div class="empty sheet">
		<strong>No unseen mail here</strong>
		Everything in this list has been seen.
		<p><button class="btn" onclick={toggleUnseenOnly}>Show all mail</button></p>
	</div>
{:else}
	<SelectAll
		{selection}
		selected={picked.length}
		total={shown.length}
		onall={() => selection.set(shown.map((t) => t.id))}
		onnone={selection.clear}
	/>
	<ThreadList threads={shown} {selection} />
{/if}

<SelectionBar
	list="other"
	archivable={false}
	threadIds={picked.map((t) => t.id)}
	accountIds={[...new Set(picked.map((t) => t.account_id))]}
	total={shown.length}
	onselectall={() => selection.set(shown.map((t) => t.id))}
	onclear={selection.clear}
	ondone={selection.stop}
/>
