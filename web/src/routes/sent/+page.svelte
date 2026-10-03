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
		api.get<ThreadSummary[]>('/threads?box=sent')
			.then((list) => (threads = list))
			.catch((e) => (error = e.message));
	});
</script>

<div class="page-head">
	<h1>Sent</h1>
	<p></p>
	{#if threads?.length}
		<p class="tools"><a class="btn small" href="/read/sent">Read all on one page</a></p>
	{/if}
</div>

{#if error}
	<p class="error" role="alert">{error}</p>
{:else if threads === null}
	<p class="empty" aria-busy="true">Loading</p>
{:else if threads.length === 0}
	<div class="empty sheet">
		<strong>Nothing sent yet</strong>
		Conversations you wrote in appear here.
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
	threadIds={picked.map((t) => t.id)}
	accountIds={[...new Set(picked.map((t) => t.account_id))]}
	total={threads?.length ?? 0}
	onselectall={() => selection.set((threads ?? []).map((t) => t.id))}
	onclear={selection.clear}
	ondone={selection.stop}
/>
