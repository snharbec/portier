<script lang="ts">
	import { page } from '$app/state';
	import { untrack } from 'svelte';
	import { app } from '#lib/app.svelte.ts';
	import SelectAll from '#lib/components/SelectAll.svelte';
	import SelectionBar from '#lib/components/SelectionBar.svelte';
	import ThreadList from '#lib/components/ThreadList.svelte';
	import { createSelection } from '#lib/selection.svelte.ts';
	import { createThreadList } from '#lib/threads.svelte.ts';

	const list = createThreadList('trash');
	const threads = $derived(list.threads ?? []);
	const selection = createSelection();
	const picked = $derived(selection.visible(threads, (t) => t.id));

	$effect(() => {
		app.tick;
		// Loading reads the list it replaces; untracked, so that storing it does not start this again.
		untrack(() => list.load(Number(page.url.searchParams.get('open'))));
	});
</script>

<div class="page-head">
	<h1>Trash</h1>
	<p>Mail you deleted. It sits in the Trash folder of your mail account until the mail server empties it. To bring one back, open it or select it and choose "Move back to Home".</p>
	{#if threads.length}
		<p class="tools">
			{#if threads.length}<a class="btn small" href="/read/trash">Read all on one page</a>{/if}
		</p>
	{/if}
</div>

{#if list.error}
	<p class="error" role="alert">{list.error}</p>
{:else if list.threads === null}
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
		onall={() => selection.set(threads.map((t) => t.id))}
		onnone={selection.clear}
	/>
	<ThreadList {threads} {selection} />
	{#if list.more}
		<p class="older">
			<button class="btn" disabled={list.busy} onclick={list.older}>Show older conversations</button>
		</p>
	{/if}
{/if}

<SelectionBar
	list="other"
	archivable={false}
	trashable={false}
	restorable
	threadIds={picked.map((t) => t.id)}
	accountIds={[...new Set(picked.map((t) => t.account_id))]}
	total={threads.length}
	onselectall={() => selection.set(threads.map((t) => t.id))}
	onclear={selection.clear}
	ondone={selection.stop}
/>

<style>
	.older {
		margin: 1rem 0 0;
		text-align: center;
	}
</style>
