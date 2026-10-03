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
		api.get<ThreadSummary[]>('/threads?box=flagged')
			.then((list) => (threads = list))
			.catch((e) => (error = e.message));
	});
</script>

<div class="page-head">
	<h1>Important</h1>
	<p>Conversations you set apart from the Inbox. Other mail programs show them as flagged.</p>
	{#if threads?.length}
		<p class="tools"><a class="btn small" href="/read/flagged">Read all on one page</a></p>
	{/if}
</div>

{#if error}
	<p class="error" role="alert">{error}</p>
{:else if threads === null}
	<p class="empty" aria-busy="true">Loading</p>
{:else if threads.length === 0}
	<div class="empty sheet">
		<strong>Nothing marked important</strong>
		Drag a conversation from the Inbox onto Important in the side bar, or press i while reading it.
	</div>
{:else}
	<SelectAll
		selected={picked.length}
		total={threads.length}
		onall={() => selection.set((threads ?? []).map((t) => t.id))}
		onnone={selection.clear}
	/>
	<ThreadList {threads} {selection} />
{/if}

<SelectionBar
	list="important"
	threadIds={picked.map((t) => t.id)}
	accountIds={[...new Set(picked.map((t) => t.account_id))]}
	total={threads?.length ?? 0}
	onselectall={() => selection.set((threads ?? []).map((t) => t.id))}
	onclear={selection.clear}
	ondone={selection.clear}
/>
