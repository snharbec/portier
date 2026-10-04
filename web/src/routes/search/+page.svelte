<script lang="ts">
	import type { SearchHit } from '#lib/api.ts';
	import { displayName, shortDate } from '#lib/format.ts';
	import FolderPicker from '#lib/components/FolderPicker.svelte';
	import SelectAll from '#lib/components/SelectAll.svelte';
	import SelectionBar from '#lib/components/SelectionBar.svelte';
	import Swipeable from '#lib/components/Swipeable.svelte';
	import { page } from '$app/state';
	import { app } from '#lib/app.svelte.ts';
	import { startReading } from '#lib/reading.ts';
	import { swipeLabel, swipeMail, type SwipeAction } from '#lib/swipe.ts';
	import { api } from '#lib/api.ts';
	import { notify, refreshCounts } from '#lib/app.svelte.ts';
	import { resultPath, runSearch, search } from '#lib/search.svelte.ts';
	import { createSelection } from '#lib/selection.svelte.ts';

	const selection = createSelection();
	const picked = $derived(selection.visible(search.hits ?? [], (h) => h.id));
	let picker: FolderPicker;

	/** The list a found mail sits in, by the names of the side bar. */
	const places: Record<SearchHit['place'], { name: string; tone: string }> = {
		home: { name: 'Home', tone: 'important' },
		flagged: { name: 'Important', tone: 'important' },
		delayed: { name: 'Delayed', tone: '' },
		feed: { name: 'Nice to know', tone: 'feed' },
		screener: { name: 'Screener', tone: '' },
		archive: { name: 'Archive', tone: '' },
		sent: { name: 'Sent', tone: '' },
		junk: { name: 'Junk', tone: 'junk' },
		trash: { name: 'Trash', tone: 'junk' }
	};

	// ---- Split view: a result opens beside the list instead of on a page of its own ----
	const hrefOf = (index: number) =>
		app.splitActive ? `/search?open=${search.hits![index].thread_id}&hit=${index}` : resultPath(index);
	/** The result whose conversation is open: the one clicked, else the first of that conversation. */
	const openIndex = $derived.by(() => {
		const open = app.splitActive ? Number(page.url.searchParams.get('open')) : 0;
		if (!open || !search.hits) return -1;
		const clicked = Number(page.url.searchParams.get('hit') ?? -1);
		if (search.hits[clicked]?.thread_id === open) return clicked;
		return search.hits.findIndex((h) => h.thread_id === open);
	});
	/** Lets the mail view move on to the next result after archiving or trashing one. */
	const opened = () => startReading([...new Set((search.hits ?? []).map((h) => h.thread_id))], '/search');

	// Opening a result reads it: its row must not stay marked as unseen.
	$effect(() => {
		const hit = search.hits?.[openIndex];
		if (hit && !hit.seen) hit.seen = true;
	});

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
				{#if search.hits.length}<a class="btn small" href="/read/search">Read all on one page</a>{/if}
			</p>
		{:else}
			<p class="tools">
				<button class="btn small" onclick={startNaming}>Save this search</button>
				{#if search.hits.length}<a class="btn small" href="/read/search">Read all on one page</a>{/if}
			</p>
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
			<dt>note:tax</dt>
			<dd>conversations whose note of yours has the word; <code>note:</code> alone finds all with a note</dd>
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
				<a
					href={hrefOf(index)}
					data-row={hit.id}
					class:open={index === openIndex}
					data-sveltekit-noscroll={app.splitActive ? '' : undefined}
					data-sveltekit-keepfocus={app.splitActive ? '' : undefined}
					onclick={opened}
				>
					<span class="top">
						<strong title={hit.from_addr}>{displayName(hit.from_name, hit.from_addr)}</strong>
						{#if places[hit.place]}
							<span class="tag place {places[hit.place].tone}" title="Found in {places[hit.place].name}"
								>{places[hit.place].name}</span
							>
						{/if}
						<time class="muted">{shortDate(hit.date)}</time>
					</span>
					<span>{hit.subject || '(no subject)'}</span>
					<span class="muted">{hit.excerpt}</span>
					{#if hit.note}<span class="note">{hit.note}</span>{/if}
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
	/* The row the arrow keys are on. Drawn inside the row: an outline around it would be cut
	   off by the sliding container. */
	a.open,
	a[data-row]:focus {
		outline: none;
		background: color-mix(in srgb, var(--important) 14%, transparent);
		box-shadow: inset 4px 0 0 var(--important);
	}
	.tools {
		align-items: center;
	}
	.note {
		justify-self: start;
		max-width: 100%;
		margin-top: 0.15rem;
		padding: 0 0.5rem;
		border-left: 3px solid var(--signal);
		border-radius: 0 6px 6px 0;
		background: color-mix(in srgb, var(--signal) 16%, var(--surface));
		font-size: 0.85rem;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.tag.place {
		flex: none;
		font-size: 0.75rem;
		border-width: 1px;
	}
	/* Lists without a colour of their own. */
	.tag.place:not(.important, .feed, .junk) {
		color: var(--ink-soft);
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
	/* The result rows only: the page's other links (buttons in the head) keep their own shape. */
	a[data-row] {
		display: grid;
		flex: 1;
		min-width: 0;
		padding: 0.8rem 1rem;
		text-decoration: none;
	}
	.top {
		display: flex;
		align-items: baseline;
		gap: 0.6rem;
	}
	.top strong {
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	time {
		margin-left: auto;
		flex: none;
		font-size: 0.85rem;
	}
</style>
