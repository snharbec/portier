<script lang="ts">
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { api, type Category, type Thread } from '#lib/api.ts';
	import { app, categoryNames, classify, refreshCounts, startDraft } from '#lib/app.svelte.ts';
	import ClassifyButtons from '#lib/components/ClassifyButtons.svelte';
	import MessageCard from '#lib/components/MessageCard.svelte';
	import { displayName } from '#lib/format.ts';

	let thread = $state<Thread | null>(null);
	let error = $state('');
	let changing = $state(false);
	/** Messages that were unread when the thread was opened stay expanded. */
	let unreadAtOpen = $state(new Set<number>());

	const id = $derived(page.params.id);

	async function load(firstLoad: boolean) {
		try {
			const loaded = await api.get<Thread>(`/threads/${id}`);
			if (firstLoad) {
				unreadAtOpen = new Set(loaded.messages.filter((m) => !m.seen).map((m) => m.id));
				if (unreadAtOpen.size) {
					await api.post(`/threads/${id}/seen`);
					refreshCounts().catch(() => {});
				}
			}
			thread = loaded;
		} catch (e) {
			error = (e as Error).message;
		}
	}

	let loadedId = '';
	$effect(() => {
		app.tick;
		const first = loadedId !== id;
		loadedId = id ?? '';
		load(first);
	});

	async function pick(category: Category) {
		if (!thread?.sender) return;
		await classify(thread.sender.id, category);
		changing = false;
	}

	const last = $derived(thread?.messages.at(-1));

	async function draft(kind: string) {
		if (last) goto(await startDraft(kind, last.id));
	}

	function onkeydown(event: KeyboardEvent) {
		const typing = (event.target as HTMLElement).closest('input, textarea, [contenteditable="true"]');
		if (typing || event.metaKey || event.ctrlKey || event.altKey) return;
		if (event.key === 'r') draft('reply');
		else if (event.key === 'a') draft('reply_all');
		else if (event.key === 'f') draft('forward');
	}
</script>

<svelte:window {onkeydown} />

{#if error}
	<p class="empty"><strong>This conversation cannot be shown</strong>{error}</p>
{:else if !thread}
	<p class="empty" aria-busy="true">Loading</p>
{:else}
	<div class="page-head">
		<h1>{thread.subject || '(no subject)'}</h1>
		{#if thread.sender}
			<p class="sender">
				{displayName(thread.sender.display_name, thread.sender.address)}
				{#if thread.sender.category}
					<span class="tag {thread.sender.category}">{categoryNames[thread.sender.category]}</span>
				{:else}
					<span class="tag">Not screened yet</span>
				{/if}
				<button class="btn small quiet" onclick={() => (changing = !changing)}>
					{thread.sender.category ? 'Change' : 'Decide'}
				</button>
			</p>
			{#if changing}
				<ClassifyButtons small current={thread.sender.category} onpick={pick} />
			{/if}
		{/if}
	</div>

	{#each thread.messages as message, index (message.id)}
		<MessageCard {message} open={index === thread.messages.length - 1 || unreadAtOpen.has(message.id)} />
	{/each}

	<p class="muted keys">Keys: <kbd>r</kbd> reply, <kbd>a</kbd> reply all, <kbd>f</kbd> forward</p>
{/if}

<style>
	.sender {
		display: flex;
		align-items: center;
		flex-wrap: wrap;
		gap: 0.6rem;
		margin-bottom: 0.6rem;
	}
	.keys {
		font-size: 0.85rem;
		text-align: center;
	}
	kbd {
		font: 600 0.8rem var(--body);
		border: 1px solid var(--line);
		border-radius: 5px;
		padding: 0 0.35rem;
		background: var(--surface);
	}
</style>
