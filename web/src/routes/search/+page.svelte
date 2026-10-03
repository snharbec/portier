<script lang="ts">
	import type { SearchHit } from '#lib/api.ts';
	import { displayName, shortDate } from '#lib/format.ts';
	import FolderPicker from '#lib/components/FolderPicker.svelte';
	import SelectAll from '#lib/components/SelectAll.svelte';
	import SelectionBar from '#lib/components/SelectionBar.svelte';
	import Swipeable from '#lib/components/Swipeable.svelte';
	import { app } from '#lib/app.svelte.ts';
	import { swipeLabel, swipeMail, type SwipeAction } from '#lib/swipe.ts';
	import { api } from '#lib/api.ts';
	import { notify, refreshCounts } from '#lib/app.svelte.ts';
	import { resultPath, runSearch, search } from '#lib/search.svelte.ts';
	import { createSelection } from '#lib/selection.svelte.ts';

	const selection = createSelection();
	const picked = $derived(selection.visible(search.hits ?? [], (h) => h.id));
	let picker: FolderPicker;

	// ---- Saving the search under a name ----
	const saved = $derived(app.searches.find((s) => s.query === search.answered.trim()));
	let naming = $state(false);
	let name = $state('');
	let saveError = $state('');

	function startNaming() {
		name = saved?.name ?? '';
		saveError = '';
		naming = true;
	}

	async function save(event: SubmitEvent) {
		event.preventDefault();
		const body = { name, query: search.answered };
		// Decided before saving: afterwards the search counts as saved either way.
		const existing = saved;
		try {
			if (existing) await api.put(`/searches/${existing.id}`, body);
			else await api.post('/searches', body);
			await refreshCounts();
			naming = false;
			notify(existing ? `Renamed to ${name.trim()}` : `Saved as ${name.trim()}`);
		} catch (e) {
			saveError = (e as Error).message;
		}
	}

	async function remove() {
		if (!saved) return;
		const removed = saved.name;
		try {
			await api.delete(`/searches/${saved.id}`);
			await refreshCounts();
			notify(`Removed ${removed} from the side bar`);
		} catch (e) {
			notify((e as Error).message);
		}
	}

	async function slide(action: SwipeAction, hit: SearchHit) {
		await swipeMail(
			action,
			{ messageIds: [hit.id], accountId: hit.account_id, unread: !hit.seen, what: '1 mail' },
			picker.choose
		);
		runSearch();
	}
</script>

<div class="page-head">
	<h1>Search</h1>
	{#if search.hits}
		<p>{search.hits.length === 200 ? 'The newest 200 mails' : search.hits.length === 1 ? '1 mail' : `${search.hits.length} mails`} for “{search.answered}”</p>
		{#if naming}
			<form class="tools" onsubmit={save}>
				<!-- svelte-ignore a11y_autofocus -->
				<input class="input name" bind:value={name} placeholder="Name for the side bar" aria-label="Name of the saved search" maxlength="40" required autofocus />
				<button class="btn small primary">{saved ? 'Rename' : 'Save search'}</button>
				<button type="button" class="btn small quiet" onclick={() => (naming = false)}>Cancel</button>
			</form>
			{#if saveError}<p class="error" role="alert">{saveError}</p>{/if}
		{:else if saved}
			<p class="tools">
				<span class="muted">Saved as “{saved.name}”</span>
				<button class="btn small" onclick={startNaming}>Rename</button>
				<button class="btn small quiet danger" onclick={remove}>Remove from side bar</button>
			</p>
		{:else}
			<p class="tools"><button class="btn small" onclick={startNaming}>Save this search</button></p>
		{/if}
	{/if}
</div>

{#if search.error}<p class="error" role="alert">{search.error}</p>{/if}

{#if search.query.trim() && !search.hits && !search.error}
	<p class="empty" aria-busy="true">Searching</p>
{:else if !search.hits && !search.error}
	<div class="sheet help">
		<p>Type in the field at the top. Words are looked up in subject, sender, recipients and text. Narrow it with:</p>
		<dl>
			<dt>from:carsten</dt>
			<dd>sender name or address contains "carsten"</dd>
			<dt>to:anna</dt>
			<dd>a recipient contains "anna"</dd>
			<dt>subject:invoice <span>or</span> title:invoice</dt>
			<dd>subject contains "invoice"</dd>
			<dt>attachment:true</dt>
			<dd>only mail with attachments; attachment:false for mail without</dd>
			<dt>received:last month</dt>
			<dd>also today, yesterday, this week, last week, this month, this year, last year</dd>
			<dt>received:01.09.2026..01.10.2026</dt>
			<dd>between two days, both included; also 2026/09/01..2026/10/01, one day alone, or an open end</dd>
		</dl>
		<p>
			Different filters narrow the search together: <code>hallo from:carsten received:last month</code>. The same
			filter given twice means either: <code>from:anna from:carsten</code> finds mail from Anna or Carsten. Put values
			with spaces in quotes: <code>from:"Carsten Meier"</code>.
		</p>
	</div>
{/if}

{#if search.hits && search.hits.length === 0}
	<p class="empty"><strong>No mail matches</strong>Try fewer or different words.</p>
{:else if search.hits}
	<SelectAll
		{selection}
		selected={picked.length}
		total={search.hits.length}
		onall={() => selection.set((search.hits ?? []).map((h) => h.id))}
		onnone={selection.clear}
	/>
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
				{#if selection.active}
				<label class="pick">
					<input
						type="checkbox"
						checked={selection.has(hit.id)}
						onchange={() => selection.toggle(hit.id)}
						aria-label="Select {hit.subject || '(no subject)'} from {displayName(hit.from_name, hit.from_addr)}"
					/>
				</label>
				{/if}
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
		selection.stop();
		runSearch();
	}}
/>

<style>
	.tools {
		align-items: center;
	}
	.input.name {
		width: min(18rem, 100%);
		padding-block: 0.3rem;
	}
	.help {
		padding: 1.1rem 1.3rem;
		max-width: 46rem;
	}
	.help p {
		margin: 0;
	}
	dl {
		display: grid;
		grid-template-columns: auto 1fr;
		gap: 0.45rem 1.25rem;
		margin: 0.9rem 0;
	}
	dt,
	code {
		font: 600 0.95rem ui-monospace, SFMono-Regular, Menlo, monospace;
		white-space: nowrap;
	}
	dt span {
		font: 400 0.9rem var(--body);
		color: var(--ink-soft);
	}
	dd {
		margin: 0;
		color: var(--ink-soft);
	}
	@media (max-width: 40rem) {
		dl {
			grid-template-columns: 1fr;
			gap: 0.1rem;
		}
		dd {
			margin-bottom: 0.5rem;
		}
		dt {
			white-space: normal;
		}
	}
	li {
		display: flex;
	}
	ul {
		list-style: none;
		margin: 0;
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
