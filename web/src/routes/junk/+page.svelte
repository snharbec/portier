<script lang="ts">
	import { api, type ThreadSummary } from '#lib/api.ts';
	import { app } from '#lib/app.svelte.ts';
	import ThreadList from '#lib/components/ThreadList.svelte';

	let threads = $state<ThreadSummary[] | null>(null);
	let error = $state('');

	$effect(() => {
		app.tick;
		api.get<ThreadSummary[]>('/threads?box=junk')
			.then((list) => (threads = list))
			.catch((e) => (error = e.message));
	});
</script>

<div class="page-head">
	<h1>Junk</h1>
	<p>Mail from senders you turned away. It also sits in the Junk folder of your mail account.</p>
</div>

{#if error}
	<p class="error" role="alert">{error}</p>
{:else if threads === null}
	<p class="empty" aria-busy="true">Loading</p>
{:else if threads.length === 0}
	<div class="empty sheet">
		<strong>No junk</strong>
		Senders you mark as junk in the Screener end up here.
	</div>
{:else}
	<ThreadList {threads} />
{/if}
