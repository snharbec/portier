<script lang="ts">
	import type { ThreadSummary } from '#lib/api.ts';
	import { app } from '#lib/app.svelte.ts';
	import { displayName, hue, shortDate } from '#lib/format.ts';
	import Avatar from './Avatar.svelte';

	let { threads }: { threads: ThreadSummary[] } = $props();

	const accountLabel = (id: number) => app.accounts.find((a) => a.id === id)?.label ?? '';
</script>

<ul class="sheet">
	{#each threads as thread (thread.id)}
		{@const who = thread.is_outgoing && thread.sender_address
			? displayName(thread.sender_name, thread.sender_address)
			: displayName(thread.from_name, thread.from_addr)}
		<li>
			<a href="/thread/{thread.id}" class:unread={thread.unread > 0}>
				<Avatar name={who} seed={thread.sender_address ?? thread.from_addr} />
				<span class="main">
					<span class="top">
						<span class="who">{who}</span>
						{#if thread.count > 1}<span class="count">{thread.count}</span>{/if}
						<time>{shortDate(thread.date)}</time>
					</span>
					<span class="subject">{thread.subject || '(no subject)'}</span>
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
		</li>
	{/each}
</ul>

<style>
	ul {
		list-style: none;
		margin: 0;
		padding: 0;
	}
	li + li {
		border-top: 1px solid var(--line);
	}
	a {
		display: flex;
		gap: 0.9rem;
		align-items: flex-start;
		padding: 0.85rem 1rem;
		text-decoration: none;
		position: relative;
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
	.you {
		font-weight: 600;
	}
	.unread .who,
	.unread .subject {
		font-weight: 700;
	}
	.unread::before {
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
