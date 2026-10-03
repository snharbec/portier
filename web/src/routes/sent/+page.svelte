<script lang="ts">
	import { api, type ThreadSummary } from '#lib/api.ts';
	import { app } from '#lib/app.svelte.ts';
	import ThreadList from '#lib/components/ThreadList.svelte';

	let threads = $state<ThreadSummary[] | null>(null);
	let error = $state('');

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
	<ThreadList {threads} />
{/if}
