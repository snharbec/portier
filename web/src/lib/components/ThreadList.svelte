<script lang="ts">
	import type { ThreadSummary } from '#lib/api.ts';
	import { app } from '#lib/app.svelte.ts';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { startDrag } from '#lib/drag.ts';
	import { displayName, hue, returnTime, shortDate } from '#lib/format.ts';
	import { startReading } from '#lib/reading.ts';
	import { runSearch, search } from '#lib/search.svelte.ts';
	import type { Selection } from '#lib/selection.svelte.ts';
	import { swipeLabel, swipeMail, type SwipeAction } from '#lib/swipe.ts';
	import Avatar from './Avatar.svelte';
	import FolderPicker from './FolderPicker.svelte';
	import Swipeable from './Swipeable.svelte';

	let {
		threads,
		selection,
		sequence
	}: {
		threads: ThreadSummary[];
		selection?: Selection;
		/** The whole list in reading order, when this is only one section of it. */
		sequence?: number[];
	} = $props();

	/** Opening a conversation remembers the list, so the email view can move on to the next one. */
	/** Where a row leads: beside the list in split view, otherwise to the conversation's own page. */
	const hrefOf = (threadId: number) =>
		app.splitActive ? `${page.url.pathname}?open=${threadId}` : `/thread/${threadId}`;
	const openId = $derived(app.splitActive ? Number(page.url.searchParams.get('open')) : 0);

	const opened = () => startReading(sequence ?? threads.map((t) => t.id), page.url.pathname);

	/** Shows every mail of the row's sender, as a search the reader can refine or save. */
	function allFrom(address: string) {
		search.query = `from:${address}`;
		runSearch();
		goto('/search');
	}

	const accountLabel = (id: number) => app.accounts.find((a) => a.id === id)?.label ?? '';

	let picker: FolderPicker;

	/** Dragging a ticked row takes every ticked conversation along; otherwise just that one. */
	function dragged(event: DragEvent, thread: ThreadSummary) {
		const ticked = selection?.has(thread.id) ? selection.all() : [];
		const row = (event.currentTarget as HTMLElement).closest('li') ?? undefined;
		startDrag(event, { threadIds: ticked.length ? ticked : [thread.id], from: page.url.pathname }, row);
	}

	function slide(action: SwipeAction, thread: ThreadSummary) {
		swipeMail(
			action,
			{ threadIds: [thread.id], accountId: thread.account_id, unread: thread.unread > 0, what: '1 conversation' },
			picker.choose
		);
	}
</script>

<ul class="sheet">
	{#each threads as thread (thread.id)}
		{@const who = thread.is_outgoing && thread.sender_address
			? displayName(thread.sender_name, thread.sender_address)
			: displayName(thread.from_name, thread.from_addr)}
		{@const address = thread.is_outgoing && thread.sender_address ? thread.sender_address : thread.from_addr}
		<li>
			<Swipeable
				left={app.swipe.left}
				right={app.swipe.right}
				label={(action) => swipeLabel(action, thread.unread > 0)}
				onaction={(action) => slide(action, thread)}
				tinted={selection?.has(thread.id)}
			>
			<span
				class="grip"
				draggable="true"
				ondragstart={(event) => dragged(event, thread)}
				title="Drag to Home, Important or Archive in the side bar"
				aria-hidden="true"
			>
				<svg viewBox="0 0 10 16"><path d="M2 2h.01M2 8h.01M2 14h.01M8 2h.01M8 8h.01M8 14h.01" /></svg>
			</span>
			{#if selection?.active}
				<label class="pick">
					<input
						type="checkbox"
						checked={selection.has(thread.id)}
						onchange={() => selection.toggle(thread.id)}
						aria-label="Select the conversation {thread.subject || '(no subject)'} with {who}"
					/>
				</label>
			{/if}
			<a
				href={hrefOf(thread.id)}
				class:unread={thread.unread > 0}
				class:open={openId === thread.id}
				data-row={thread.id}
				data-sveltekit-noscroll={app.splitActive ? '' : undefined}
				data-sveltekit-keepfocus={app.splitActive ? '' : undefined}
				onclick={opened}
			>
				<Avatar
					name={who}
					size={36}
					seed={thread.sender_address ?? thread.from_addr}
					address={thread.is_outgoing && thread.sender_address ? thread.sender_address : thread.from_addr}
				/>
				<span class="main">
					<span class="top">
						<span class="who" title={thread.is_outgoing && thread.sender_address ? thread.sender_address : thread.from_addr}
							>{who}</span
						>
						{#if thread.count > 1}<span class="count">{thread.count}</span>{/if}
						<time>{shortDate(thread.date)}</time>
					</span>
					<span class="subject">{thread.subject || '(no subject)'}</span>
					{#if thread.snoozed_until}
						<span class="returns">Returns {returnTime(thread.snoozed_until)}</span>
					{/if}
					<span class="snippet">
						{#if thread.is_outgoing}<span class="you">You:</span>{/if}
						{thread.snippet}
					</span>
				</span>
				{#if app.accounts.length > 1}
					<span class="account" style="--h: {hue(String(thread.account_id * 97))}" title={accountLabel(thread.account_id)}
					></span>
				{/if}
			</a>
			{#if address}
				<button class="from" onclick={() => allFrom(address)} title="All mail from {who}" aria-label="All mail from {who}">
					<svg viewBox="0 0 24 24" aria-hidden="true"
						><path d="M9.5 11a3.5 3.5 0 1 0 0-7 3.5 3.5 0 0 0 0 7zM3 19.5c0-3 2.9-5 6.5-5 1 0 2 .2 2.8.5M17 13.5a3 3 0 1 0 0 6 3 3 0 0 0 0-6zM19.200 18.700 21.500 21" /></svg
					>
				</button>
			{/if}
			</Swipeable>
		</li>
	{/each}
</ul>

<FolderPicker bind:this={picker} />

<style>
	ul {
		list-style: none;
		margin: 0;
		padding: 0;
		/* Rows lay themselves out by the list's own width, which is narrow in split view. */
		container: list / inline-size;
	}
	li {
		display: flex;
		position: relative;
	}
	li + li {
		border-top: 1px solid var(--line);
	}
	a {
		display: flex;
		gap: 0.9rem;
		align-items: flex-start;
		flex: 1;
		min-width: 0;
		padding: 0.85rem 1rem;
		text-decoration: none;
	}
	a:hover {
		background: color-mix(in srgb, var(--important) 6%, transparent);
	}
	.main {
		display: grid;
		min-width: 0;
		flex: 1;
	}
	.top {
		display: flex;
		align-items: baseline;
		gap: 0.5rem;
	}
	.who {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.count {
		font-size: 0.8rem;
		color: var(--ink-soft);
		border: 1px solid var(--line);
		border-radius: 999px;
		padding: 0 0.4rem;
	}
	time {
		margin-left: auto;
		flex: none;
		font-size: 0.85rem;
		color: var(--ink-soft);
	}
	.subject,
	.snippet {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.snippet {
		color: var(--ink-soft);
		font-size: 0.925rem;
	}
	.returns {
		font-size: 0.85rem;
		font-weight: 600;
		color: var(--feed);
	}
	/* Handle for dragging a row to the side bar; only where the side bar and a mouse exist. */
	.grip {
		display: none;
	}
	@media (min-width: 69rem) and (hover: hover) {
		.grip {
			flex: none;
			display: grid;
			place-items: center;
			width: 1.1rem;
			margin-right: -0.55rem;
			cursor: grab;
			opacity: 0;
		}
		li:hover .grip {
			opacity: 0.55;
		}
		.grip:hover {
			opacity: 1 !important;
		}
		.grip svg {
			width: 0.6rem;
			fill: none;
			stroke: var(--ink-soft);
			stroke-width: 2.4;
			stroke-linecap: round;
		}
	}
	/* "All mail from this sender": at the end of the row, shown when the row is pointed at. */
	.from {
		flex: none;
		display: grid;
		place-items: center;
		width: 2.4rem;
		border: 0;
		background: none;
		color: var(--ink-soft);
		cursor: pointer;
		opacity: 0;
	}
	.from svg {
		width: 1.25rem;
		height: 1.25rem;
		fill: none;
		stroke: currentColor;
		stroke-width: 1.8;
		stroke-linecap: round;
		stroke-linejoin: round;
	}
	li:hover .from,
	.from:focus-visible {
		opacity: 0.7;
	}
	.from:hover {
		opacity: 1 !important;
		color: var(--important);
	}
	/* Nothing to point with: always there, quietly. */
	@media (hover: none) {
		.from {
			opacity: 0.55;
		}
	}
	a.open {
		background: color-mix(in srgb, var(--important) 12%, transparent);
		box-shadow: inset 3px 0 0 var(--important);
	}
	/* A wide list: sender, subject with the start of the text, and date on a single line, so a
	   screenful holds far more conversations and the eye runs along one row. */
	@container list (min-width: 52rem) {
		a {
			align-items: center;
			padding-block: 0.5rem;
		}
		.main {
			display: flex;
			align-items: baseline;
			gap: 0.75rem;
		}
		.top {
			display: contents;
		}
		.who {
			order: 1;
			flex: 0 0 13rem;
		}
		.count {
			order: 2;
			flex: none;
		}
		.subject {
			order: 3;
			flex: 0 1 auto;
			max-width: 45%;
		}
		.snippet {
			order: 4;
			flex: 1 1 0;
			min-width: 0;
		}
		.returns {
			order: 5;
			flex: none;
		}
		time {
			order: 6;
			margin-left: 0;
		}
	}
	.you {
		font-weight: 600;
	}
	.unread .who,
	.unread .subject {
		font-weight: 700;
	}
	a.unread::before {
		content: '';
		position: absolute;
		left: 0;
		top: 0.6rem;
		bottom: 0.6rem;
		width: 4px;
		border-radius: 0 4px 4px 0;
		background: var(--important);
	}
	.account {
		flex: none;
		align-self: center;
		width: 0.6rem;
		height: 0.6rem;
		border-radius: 2px;
		background: hsl(var(--h) 65% 55%);
	}
</style>
