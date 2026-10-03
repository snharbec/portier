<script lang="ts">
	import { api, type SearchHit } from '#lib/api.ts';
	import { displayName, shortDate } from '#lib/format.ts';
	import FolderPicker from '#lib/components/FolderPicker.svelte';
	import SelectionBar from '#lib/components/SelectionBar.svelte';
	import Swipeable from '#lib/components/Swipeable.svelte';
	import { app } from '#lib/app.svelte.ts';
	import { swipeLabel, swipeMail, type SwipeAction } from '#lib/swipe.ts';
	import { rememberSearch, resultPath, search } from '#lib/search.svelte.ts';
	import { createSelection } from '#lib/selection.svelte.ts';

	let error = $state('');
	const selection = createSelection();
	const picked = $derived(selection.visible(search.hits ?? [], (h) => h.id));
	let picker: FolderPicker;

	async function slide(action: SwipeAction, hit: SearchHit) {
		await swipeMail(
			action,
			{ messageIds: [hit.id], accountId: hit.account_id, unread: !hit.seen, what: '1 mail' },
			picker.choose
		);
		run();
	}
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
				<Swipeable
					left={app.swipe.left}
					right={app.swipe.right}
					label={(action) => swipeLabel(action, !hit.seen)}
					onaction={(action) => slide(action, hit)}
					tinted={selection.has(hit.id)}
				>
				<label class="pick">
					<input
						type="checkbox"
						checked={selection.has(hit.id)}
						onchange={() => selection.toggle(hit.id)}
						aria-label="Select {hit.subject || '(no subject)'} from {displayName(hit.from_name, hit.from_addr)}"
					/>
				</label>
				<a href={resultPath(index)}>
					<span class="top">
						<strong title={hit.from_addr}>{displayName(hit.from_name, hit.from_addr)}</strong>
						<time class="muted">{shortDate(hit.date)}</time>
					</span>
					<span>{hit.subject || '(no subject)'}</span>
					<span class="muted">{hit.excerpt}</span>
				</a>
				</Swipeable>
			</li>
		{/each}
	</ul>
{/if}

<FolderPicker bind:this={picker} />

<SelectionBar
	messageIds={picked.map((h) => h.id)}
	accountIds={[...new Set(picked.map((h) => h.account_id))]}
	total={search.hits?.length ?? 0}
	onselectall={() => selection.set((search.hits ?? []).map((h) => h.id))}
	onclear={selection.clear}
	ondone={() => {
		selection.clear();
		run();
	}}
/>

<style>
	li {
		display: flex;
	}
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
		flex: 1;
		min-width: 0;
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
