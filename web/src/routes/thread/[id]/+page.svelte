<script lang="ts">
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { api, type Category, type Thread } from '#lib/api.ts';
	import { app, categoryNames, classify, refreshCounts, startDraft } from '#lib/app.svelte.ts';
	import ClassifyButtons from '#lib/components/ClassifyButtons.svelte';
	import MessageCard from '#lib/components/MessageCard.svelte';
	import { displayName } from '#lib/format.ts';
	import { resultPath, search } from '#lib/search.svelte.ts';

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

	/** Position in the last search's results, when this conversation was opened from there. */
	const hit = $derived.by(() => {
		const raw = page.url.searchParams.get('hit');
		const index = raw === null ? -1 : Number(raw);
		const valid = search.hits && Number.isInteger(index) && index >= 0 && index < search.hits.length;
		return valid && String(search.hits![index].thread_id) === id ? index : -1;
	});
	const hitMessage = $derived(hit >= 0 ? search.hits![hit].id : null);

	function toResult(index: number) {
		if (search.hits && index >= 0 && index < search.hits.length) goto(resultPath(index));
	}

	async function draft(kind: string) {
		if (last) goto(await startDraft(kind, last.id));
	}

	function onkeydown(event: KeyboardEvent) {
		const typing = (event.target as HTMLElement).closest('input, textarea, [contenteditable="true"]');
		if (typing || event.metaKey || event.ctrlKey || event.altKey) return;
		if (document.querySelector('dialog[open]')) return;
		if (event.key === 'r') draft('reply');
		else if (event.key === 'a') draft('reply_all');
		else if (event.key === 'f') draft('forward');
		else if (hit >= 0 && event.key === 'p') toResult(hit - 1);
		else if (hit >= 0 && event.key === 'n') toResult(hit + 1);
	}
</script>

<svelte:window {onkeydown} />

{#if error}
	<p class="empty"><strong>This conversation cannot be shown</strong>{error}</p>
{:else if !thread}
	<p class="empty" aria-busy="true">Loading</p>
{:else}
	{#if hit >= 0 && search.hits}
		<nav class="results" aria-label="Search results">
			<a class="btn small" href="/search">Back to results</a>
			<span class="muted">Result {hit + 1} of {search.hits.length} for “{search.query}”</span>
			<button class="btn small" onclick={() => toResult(hit - 1)} disabled={hit === 0}>Previous</button>
			<button class="btn small" onclick={() => toResult(hit + 1)} disabled={hit === search.hits.length - 1}>
				Next
			</button>
		</nav>
	{/if}
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
		<MessageCard
			{message}
			open={index === thread.messages.length - 1 || unreadAtOpen.has(message.id) || message.id === hitMessage}
		/>
	{/each}

	<p class="muted keys">
		Keys: <kbd>r</kbd> reply, <kbd>a</kbd> reply all, <kbd>f</kbd> forward{#if hit >= 0}, <kbd>p</kbd> previous result,
			<kbd>n</kbd> next result{/if}
	</p>
{/if}

<style>
	.results {
		display: flex;
		align-items: center;
		flex-wrap: wrap;
		gap: 0.5rem;
		margin-top: 1.25rem;
		padding: 0.5rem 0.6rem;
		background: var(--surface);
		border: 1px solid var(--line);
		border-radius: 999px;
	}
	.results span {
		flex: 1;
		min-width: 8rem;
		font-size: 0.9rem;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
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
