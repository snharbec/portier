<script lang="ts">
	import { api, type Message } from '#lib/api.ts';
	import { app } from '#lib/app.svelte.ts';
	import MessageCard from '#lib/components/MessageCard.svelte';

	let messages = $state<Message[] | null>(null);
	let done = $state(false);
	let busy = $state(false);
	let error = $state('');

	async function load(reset: boolean) {
		busy = true;
		try {
			const offset = reset ? 0 : (messages?.length ?? 0);
			const page = await api.get<Message[]>(`/feed?offset=${offset}`);
			messages = reset ? page : [...(messages ?? []), ...page];
			done = page.length < 20;
		} catch (e) {
			error = (e as Error).message;
		} finally {
			busy = false;
		}
	}

	$effect(() => {
		app.tick;
		load(true);
	});
</script>

<div class="page-head">
	<h1>Nice to know</h1>
	<p>Newsletters and updates, already open. Scroll through them when you have a moment.</p>
</div>

{#if error}<p class="error" role="alert">{error}</p>{/if}

{#if messages === null}
	<p class="empty" aria-busy="true">Loading</p>
{:else if messages.length === 0}
	<div class="empty sheet">
		<strong>Nothing to read yet</strong>
		Mark a sender as nice to know in the Screener and their mail collects here.
	</div>
{:else}
	{#each messages as message (message.id)}
		<MessageCard {message} showSubject actions={false} />
	{/each}
	{#if !done}
		<p class="more"><button class="btn" disabled={busy} onclick={() => load(false)}>Show older mail</button></p>
	{/if}
{/if}

<style>
	.more {
		text-align: center;
	}
</style>
