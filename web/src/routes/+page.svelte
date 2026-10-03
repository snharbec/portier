<script lang="ts">
	import { api, type ThreadSummary } from '#lib/api.ts';
	import { app } from '#lib/app.svelte.ts';
	import SelectionBar from '#lib/components/SelectionBar.svelte';
	import ThreadList from '#lib/components/ThreadList.svelte';
	import { createSelection } from '#lib/selection.svelte.ts';

	let threads = $state<ThreadSummary[] | null>(null);
	let error = $state('');
	const selection = createSelection();
	const picked = $derived(selection.visible(threads ?? [], (t) => t.id));

	$effect(() => {
		app.tick;
		api.get<ThreadSummary[]>('/threads?box=important')
			.then((list) => (threads = list))
			.catch((e) => (error = e.message));
	});

	const fresh = $derived(threads?.filter((t) => t.unread > 0) ?? []);
	const seen = $derived(threads?.filter((t) => t.unread === 0) ?? []);
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
	<h1>Important</h1>
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
{:else if threads.length === 0}
	<div class="empty sheet">
		<strong>Nothing important yet</strong>
		Mail shows up here once you mark its sender as important in the Screener.
	</div>
{:else}
	{#if fresh.length}
		<h2 class="section-title">New for you</h2>
		<ThreadList threads={fresh} {selection} />
	{/if}
	{#if seen.length}
		<h2 class="section-title">Previously seen</h2>
		<ThreadList threads={seen} {selection} />
	{/if}
{/if}

<SelectionBar
	threadIds={picked.map((t) => t.id)}
	accountIds={[...new Set(picked.map((t) => t.account_id))]}
	total={threads?.length ?? 0}
	onselectall={() => selection.set((threads ?? []).map((t) => t.id))}
	onclear={selection.clear}
	ondone={selection.clear}
/>

<style>
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
