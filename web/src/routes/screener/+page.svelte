<script lang="ts">
	import { api, type Category, type Message, type ScreenerEntry } from '#lib/api.ts';
	import { app, classify } from '#lib/app.svelte.ts';
	import Avatar from '#lib/components/Avatar.svelte';
	import ClassifyButtons from '#lib/components/ClassifyButtons.svelte';
	import MessageBody from '#lib/components/MessageBody.svelte';
	import { displayName, shortDate } from '#lib/format.ts';

	let entries = $state<ScreenerEntry[] | null>(null);
	let error = $state('');
	let preview = $state<Record<number, Message>>({});

	$effect(() => {
		app.tick;
		api.get<ScreenerEntry[]>('/screener')
			.then((list) => (entries = list))
			.catch((e) => (error = e.message));
	});

	async function decide(entry: ScreenerEntry, category: Category) {
		// Remove at once; the list reloads when the server confirms.
		entries = entries?.filter((e) => e.id !== entry.id) ?? null;
		try {
			await classify(entry.id, category);
		} catch (e) {
			error = (e as Error).message;
		}
	}

	async function togglePreview(entry: ScreenerEntry) {
		if (preview[entry.id]) {
			delete preview[entry.id];
			return;
		}
		preview[entry.id] = await api.get<Message>(`/messages/${entry.message_id}`);
	}

	const keys: Record<string, Category> = { i: 'important', k: 'feed', j: 'junk' };
	function onkeydown(event: KeyboardEvent) {
		const typing = (event.target as HTMLElement).closest('input, textarea, [contenteditable="true"]');
		const category = keys[event.key];
		if (!typing && category && entries?.length && !event.metaKey && !event.ctrlKey) decide(entries[0], category);
	}
</script>

<svelte:window {onkeydown} />

<div class="page-head">
	<h1>Screener</h1>
	<p>
		These people wrote to you for the first time. Choose where their mail goes from now on. They are never told what
		you picked.
	</p>
</div>

{#if error}<p class="error" role="alert">{error}</p>{/if}

{#if entries === null}
	<p class="empty" aria-busy="true">Loading</p>
{:else if entries.length === 0}
	<div class="empty sheet">
		<strong>Nobody is waiting</strong>
		Every sender in your inbox has a place.
	</div>
{:else}
	<p class="muted keys">
		Keys for the first sender: <kbd>i</kbd> inbox, <kbd>k</kbd> nice to know, <kbd>j</kbd> junk
	</p>
	{#each entries as entry (entry.id)}
		{@const who = displayName(entry.display_name, entry.address)}
		<section class="sheet card">
			<div class="who">
				<Avatar name={who} seed={entry.address} size={52} />
				<div>
					<h2>{who}</h2>
					<span class="muted">{entry.address}</span>
				</div>
				<time class="muted">{shortDate(entry.date)}</time>
			</div>
			<button class="latest" onclick={() => togglePreview(entry)} aria-expanded={!!preview[entry.id]}>
				<strong>{entry.subject || '(no subject)'}</strong>
				<span>{entry.snippet}</span>
				{#if entry.count > 1}<em>and {entry.count - 1} more</em>{/if}
			</button>
			{#if preview[entry.id]}
				<div class="preview"><MessageBody message={preview[entry.id]} /></div>
			{/if}
			<ClassifyButtons onpick={(category) => decide(entry, category)} />
		</section>
	{/each}
{/if}

<style>
	.keys {
		font-size: 0.875rem;
		margin: 0 0 0.75rem;
	}
	kbd {
		font: 600 0.8rem var(--body);
		border: 1px solid var(--line);
		border-radius: 5px;
		padding: 0 0.35rem;
		background: var(--surface);
	}
	.card {
		padding: 1.1rem 1.2rem 1.2rem;
		margin-bottom: 0.9rem;
		display: grid;
		gap: 0.9rem;
	}
	.who {
		display: flex;
		align-items: center;
		gap: 0.9rem;
	}
	.who time {
		margin-left: auto;
		font-size: 0.85rem;
	}
	.latest {
		display: grid;
		gap: 0.15rem;
		text-align: left;
		padding: 0.7rem 0.9rem;
		border: 0;
		border-left: 3px solid var(--line);
		background: var(--paper);
		border-radius: 0 9px 9px 0;
		cursor: pointer;
	}
	.latest span {
		color: var(--ink-soft);
		display: -webkit-box;
		-webkit-line-clamp: 2;
		line-clamp: 2;
		-webkit-box-orient: vertical;
		overflow: hidden;
	}
	.latest em {
		font-size: 0.85rem;
		color: var(--ink-soft);
	}
</style>
