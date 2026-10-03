<script lang="ts">
	import { api, type SearchHit } from '#lib/api.ts';
	import { displayName, shortDate } from '#lib/format.ts';
	import { rememberSearch, resultPath, search } from '#lib/search.svelte.ts';

	let error = $state('');
	let timer: ReturnType<typeof setTimeout>;

	function oninput() {
		clearTimeout(timer);
		timer = setTimeout(run, 250);
	}

	async function run() {
		if (!search.query.trim()) {
			search.hits = null;
		} else {
			try {
				search.hits = await api.get<SearchHit[]>(`/search?q=${encodeURIComponent(search.query)}`);
				error = '';
			} catch (e) {
				error = (e as Error).message;
			}
		}
		rememberSearch();
	}
</script>

<div class="page-head">
	<h1>Search</h1>
</div>
<!-- svelte-ignore a11y_autofocus -->
<input class="input" type="search" placeholder="Words, names or addresses" bind:value={search.query} {oninput} autofocus />

{#if error}<p class="error" role="alert">{error}</p>{/if}
{#if search.hits && search.hits.length === 0}
	<p class="empty"><strong>No mail matches</strong>Try fewer or different words.</p>
{:else if search.hits}
	<ul class="sheet">
		{#each search.hits as hit, index (hit.id)}
			<li>
				<a href={resultPath(index)}>
					<span class="top">
						<strong>{displayName(hit.from_name, hit.from_addr)}</strong>
						<time class="muted">{shortDate(hit.date)}</time>
					</span>
					<span>{hit.subject || '(no subject)'}</span>
					<span class="muted">{hit.excerpt}</span>
				</a>
			</li>
		{/each}
	</ul>
{/if}

<style>
	ul {
		list-style: none;
		margin: 1rem 0 0;
		padding: 0;
	}
	li + li {
		border-top: 1px solid var(--line);
	}
	a {
		display: grid;
		padding: 0.8rem 1rem;
		text-decoration: none;
	}
	.top {
		display: flex;
		justify-content: space-between;
		gap: 1rem;
	}
	time {
		font-size: 0.85rem;
	}
</style>
