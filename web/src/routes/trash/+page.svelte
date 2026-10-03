<script lang="ts">
	import { api, type ThreadSummary } from '#lib/api.ts';
	import { app } from '#lib/app.svelte.ts';
	import SelectAll from '#lib/components/SelectAll.svelte';
	import SelectionBar from '#lib/components/SelectionBar.svelte';
	import ThreadList from '#lib/components/ThreadList.svelte';
	import { createSelection } from '#lib/selection.svelte.ts';

	let threads = $state<ThreadSummary[] | null>(null);
	let error = $state('');
	const selection = createSelection();
	const picked = $derived(selection.visible(threads ?? [], (t) => t.id));

	$effect(() => {
		app.tick;
		api.get<ThreadSummary[]>('/threads?box=trash')
			.then((list) => (threads = list))
			.catch((e) => (error = e.message));
	});
</script>

<div class="page-head">
	<h1>Trash</h1>
	<p>Mail you deleted. It sits in the Trash folder of your mail account until the mail server empties it. To bring one back, open it or select it and choose "Move back to Inbox".</p>
	{#if threads?.length}
		<p class="tools"><a class="btn small" href="/read/trash">Read all on one page</a></p>
	{/if}
</div>

{#if error}
	<p class="error" role="alert">{error}</p>
{:else if threads === null}
	<p class="empty" aria-busy="true">Loading</p>
{:else if threads.length === 0}
	<div class="empty sheet">
		<strong>The Trash is empty</strong>
		Mail you move to the Trash is listed here.
	</div>
{:else}
	<SelectAll
		{selection}
		selected={picked.length}
		total={threads.length}
		onall={() => selection.set((threads ?? []).map((t) => t.id))}
		onnone={selection.clear}
	/>
	<ThreadList {threads} {selection} />
{/if}

<SelectionBar
	list="other"
	archivable={false}
	trashable={false}
	restorable
	threadIds={picked.map((t) => t.id)}
	accountIds={[...new Set(picked.map((t) => t.account_id))]}
	total={threads?.length ?? 0}
	onselectall={() => selection.set((threads ?? []).map((t) => t.id))}
	onclear={selection.clear}
	ondone={selection.stop}
/>
