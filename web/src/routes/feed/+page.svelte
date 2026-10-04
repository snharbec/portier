<script lang="ts">
	import { page } from '$app/state';
	import { untrack } from 'svelte';
	import { app } from '#lib/app.svelte.ts';
	import SelectAll from '#lib/components/SelectAll.svelte';
	import SelectionBar from '#lib/components/SelectionBar.svelte';
	import ThreadList from '#lib/components/ThreadList.svelte';
	import { createSelection } from '#lib/selection.svelte.ts';
	import { createThreadList } from '#lib/threads.svelte.ts';

	const list = createThreadList('feed', true);
	const threads = $derived(list.threads ?? []);
	const selection = createSelection();
	const picked = $derived(selection.visible(threads, (t) => t.id));

	$effect(() => {
		app.tick;
		// Loading reads the list it replaces; untracked, so that storing it does not start this again.
		untrack(() => list.load(Number(page.url.searchParams.get('open'))));
	});

	function narrow() {
		selection.stop();
		list.toggleUnseenOnly();
	}
</script>

<div class="page-head">
	<h1>Nice to know</h1>
	<p>Newsletters and updates from senders you filed here.</p>
	{#if threads.length || list.unseenOnly}
		<p class="tools">
			<button class="btn small" aria-pressed={list.unseenOnly} onclick={narrow}>Unseen only</button>
			{#if threads.length}<a class="btn small" href="/read/feed">Read all on one page</a>{/if}
		</p>
	{/if}
</div>

{#if list.error}
	<p class="error" role="alert">{list.error}</p>
{:else if list.threads === null}
	<p class="empty" aria-busy="true">Loading</p>
{:else if threads.length === 0 && list.unseenOnly}
	<div class="empty sheet">
		<strong>No unseen mail here</strong>
		Everything in this list has been seen.
		<p><button class="btn" onclick={narrow}>Show all mail</button></p>
	</div>
{:else if threads.length === 0}
	<div class="empty sheet">
		<strong>Nothing to read yet</strong>
		Mark a sender as nice to know in the Screener and their mail collects here.
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
