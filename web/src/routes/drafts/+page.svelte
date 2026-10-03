<script lang="ts">
	import { api, type Draft } from '#lib/api.ts';
	import { app, refreshCounts, rememberDraftReturn } from '#lib/app.svelte.ts';
	import { shortDate } from '#lib/format.ts';

	let drafts = $state<Draft[] | null>(null);
	let error = $state('');

	async function load() {
		try {
			drafts = await api.get<Draft[]>('/drafts');
		} catch (e) {
			error = (e as Error).message;
		}
	}
	$effect(() => {
		app.tick;
		load();
	});

	async function discard(id: number) {
		await api.delete(`/drafts/${id}`);
		await Promise.all([load(), refreshCounts()]);
	}
</script>

<div class="page-head"><h1>Drafts</h1></div>

{#if error}<p class="error" role="alert">{error}</p>{/if}
{#if drafts === null}
	<p class="empty" aria-busy="true">Loading</p>
{:else if drafts.length === 0}
	<div class="empty sheet">
		<strong>No drafts</strong>
		Messages you start and do not send are kept here.
	</div>
{:else}
	<ul class="sheet">
		{#each drafts as draft (draft.id)}
			<li>
				<a href="/compose/{draft.id}" onclick={() => rememberDraftReturn(draft.id)}>
					<strong>{draft.subject || '(no subject)'}</strong>
					<span class="muted">{draft.to_addrs ? `to ${draft.to_addrs}` : 'no recipient yet'}</span>
				</a>
				<time class="muted">{shortDate(draft.updated_at)}</time>
				<button class="btn small danger" onclick={() => discard(draft.id)}>Discard</button>
			</li>
		{/each}
	</ul>
{/if}

<style>
	ul {
		list-style: none;
		margin: 0;
		padding: 0;
	}
	li {
		display: flex;
		align-items: center;
		gap: 1rem;
		padding: 0.8rem 1rem;
	}
	li + li {
		border-top: 1px solid var(--line);
	}
	a {
		display: grid;
		flex: 1;
		min-width: 0;
		text-decoration: none;
	}
	time {
		font-size: 0.85rem;
	}
</style>
