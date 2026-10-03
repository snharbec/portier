<script lang="ts">
	import { api, type ThreadSummary } from '#lib/api.ts';
	import { app } from '#lib/app.svelte.ts';
	import SelectAll from '#lib/components/SelectAll.svelte';
	import SelectionBar from '#lib/components/SelectionBar.svelte';
	import ThreadList from '#lib/components/ThreadList.svelte';
	import { createSelection } from '#lib/selection.svelte.ts';

	/** Conversations of the Inbox, and those set apart as Important (flagged). */
	let threads = $state<ThreadSummary[] | null>(null);
	let flagged = $state<ThreadSummary[] | null>(null);
	let error = $state('');
	const selection = createSelection();

	$effect(() => {
		app.tick;
		Promise.all([
			api.get<ThreadSummary[]>('/threads?box=important'),
			api.get<ThreadSummary[]>('/threads?box=flagged')
		])
			.then(([inbox, important]) => {
				threads = inbox;
				flagged = important;
			})
			.catch((e) => (error = e.message));
	});

	const unseen = $derived(threads?.filter((t) => t.unread > 0) ?? []);
	const seen = $derived(threads?.filter((t) => t.unread === 0) ?? []);
	const areas = $derived([
		{ id: 'unseen', title: 'Unseen', list: unseen, empty: 'No unseen messages. Area is empty.' },
		{ id: 'important', title: 'Important', list: flagged ?? [], empty: 'No flagged messages. Area is empty.' },
		{ id: 'seen', title: 'Seen', list: seen, empty: 'No seen messages. Area is empty.' }
	]);
	const shown = $derived(areas.flatMap((area) => area.list));
	// Reading order follows the page: unseen, important, seen.
	const sequence = $derived(shown.map((t) => t.id));
	const picked = $derived(selection.visible(shown, (t) => t.id));

	// Which areas are folded away; kept in this browser.
	const KEY = 'emscreen.inbox.collapsed';
	let collapsed = $state<Record<string, boolean>>(restore());

	function restore(): Record<string, boolean> {
		try {
			return JSON.parse(localStorage.getItem(KEY) ?? '{}');
		} catch {
			return {};
		}
	}

	function toggled(id: string, open: boolean) {
		collapsed[id] = !open;
		try {
			localStorage.setItem(KEY, JSON.stringify(collapsed));
		} catch {
			// Without storage the areas simply start open next time.
		}
	}
</script>

{#if app.counts.screener > 0}
	<a class="ticket" href="/screener">
		<span class="number">{app.counts.screener}</span>
		<span>
			<strong>{app.counts.screener === 1 ? 'new sender is' : 'new senders are'} waiting at the door</strong>
			<span>Decide once per sender where their mail goes.</span>
		</span>
		<span class="go">Open the Screener</span>
	</a>
{/if}

<div class="page-head">
	<h1>Inbox</h1>
	{#if threads?.length}
		<p class="tools"><a class="btn small" href="/read/important">Read all on one page</a></p>
	{/if}
</div>

{#if error}
	<p class="error" role="alert">{error}</p>
{:else if threads === null}
	<p class="empty" aria-busy="true">Loading</p>
{:else if app.accounts.length === 0}
	<div class="empty sheet">
		<strong>Connect a mail account to begin</strong>
		Email Screen reads mail from accounts you already have.
		<p><a class="btn primary" href="/settings">Add a mail account</a></p>
	</div>
{:else}
	{#if shown.length}
		<SelectAll
			selected={picked.length}
			total={shown.length}
			onall={() => selection.set(shown.map((t) => t.id))}
			onnone={selection.clear}
		/>
	{/if}
	{#each areas as area (area.id)}
		<details
			class="area"
			open={!collapsed[area.id]}
			ontoggle={(event) => toggled(area.id, event.currentTarget.open)}
		>
			<summary>
				<svg viewBox="0 0 12 8" aria-hidden="true"><path d="M1 1.5 6 6.5l5-5" /></svg>
				<h2>{area.title}</h2>
				<span class="count">{area.list.length}</span>
			</summary>
			{#if area.list.length}
				<ThreadList threads={area.list} {selection} {sequence} />
			{:else}
				<p class="nothing sheet">{area.empty}</p>
			{/if}
		</details>
	{/each}
{/if}

<SelectionBar
	list="mixed"
	threadIds={picked.map((t) => t.id)}
	accountIds={[...new Set(picked.map((t) => t.account_id))]}
	total={shown.length}
	onselectall={() => selection.set(shown.map((t) => t.id))}
	onclear={selection.clear}
	ondone={selection.clear}
/>

<style>
	.area {
		margin-top: 1.25rem;
	}
	.area summary {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		width: fit-content;
		padding: 0.25rem 0.6rem 0.25rem 0.3rem;
		margin-bottom: 0.4rem;
		border-radius: 999px;
		cursor: pointer;
		list-style: none;
		user-select: none;
	}
	.area summary::-webkit-details-marker {
		display: none;
	}
	.area summary:hover {
		background: var(--surface);
	}
	.area summary svg {
		width: 0.7rem;
		fill: none;
		stroke: var(--ink-soft);
		stroke-width: 2;
		stroke-linecap: round;
		transform: rotate(-90deg);
		transition: transform 0.15s;
	}
	.area[open] summary svg {
		transform: none;
	}
	.area h2 {
		font: 700 1.05rem var(--display);
	}
	.area .count {
		font-size: 0.85rem;
		color: var(--ink-soft);
	}
	.nothing {
		margin: 0;
		padding: 1.1rem 1.2rem;
		color: var(--ink-soft);
	}
	.ticket {
		display: flex;
		align-items: center;
		gap: 1rem;
		margin-top: 1.5rem;
		padding: 0.9rem 1.2rem;
		background: var(--signal);
		color: var(--signal-ink);
		border-radius: var(--radius);
		text-decoration: none;
		/* A torn-ticket edge: the one loud object on an otherwise quiet page. */
		mask: radial-gradient(circle 9px at 0 50%, transparent 98%, #000) left / 51% 100% no-repeat,
			radial-gradient(circle 9px at 100% 50%, transparent 98%, #000) right / 51% 100% no-repeat;
	}
	.ticket .number {
		font: 800 2.6rem/1 var(--display);
		padding-left: 0.6rem;
	}
	.ticket strong {
		display: block;
		font: 700 1.1rem var(--display);
	}
	.ticket .go {
		margin-left: auto;
		padding-right: 0.6rem;
		font-weight: 700;
		text-decoration: underline;
		text-underline-offset: 3px;
		white-space: nowrap;
	}
	@media (max-width: 34rem) {
		.ticket .go {
			display: none;
		}
	}
</style>
