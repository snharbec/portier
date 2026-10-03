<script lang="ts">
	import '../app.css';
	import { goto } from '$app/navigation';
	import { page } from '$app/state';
	import { app, loadSession, logout, startDraft } from '#lib/app.svelte.ts';
	import Login from '#lib/components/Login.svelte';
	import { onMount } from 'svelte';

	let { children } = $props();
	let menuOpen = $state(false);
	let loadError = $state('');

	onMount(() => {
		loadSession().catch((e) => (loadError = e.message));
	});

	const places = $derived([
		{ href: '/', label: 'Important', hint: 'Mail from people you let in', key: '1', badge: app.counts.unread_important },
		{ href: '/screener', label: 'Screener', hint: 'New senders waiting for your decision', key: '2', badge: app.counts.screener },
		{ href: '/feed', label: 'Nice to know', hint: 'Newsletters and updates, ready to read', key: '3', badge: 0 },
		{ href: '/files', label: 'Attachments', hint: 'Files from the last four weeks', key: '4', badge: 0 },
		{ href: '/sent', label: 'Sent', hint: '', key: '', badge: 0 },
		{ href: '/drafts', label: 'Drafts', hint: '', key: '', badge: app.counts.drafts },
		{ href: '/junk', label: 'Junk', hint: 'Senders you turned away', key: '', badge: 0 },
		{ href: '/settings', label: 'Settings', hint: 'Mail accounts, senders, users', key: '', badge: 0 }
	]);
	const here = $derived(places.find((p) => p.href === page.url.pathname)?.label ?? 'emscreen');

	async function write() {
		goto(await startDraft('new'));
	}

	function onkeydown(event: KeyboardEvent) {
		const target = event.target as HTMLElement;
		const typing = target.closest('input, textarea, select, [contenteditable="true"]');
		if (event.key === 'Escape') menuOpen = false;
		// An open dialog (the attachment viewer) owns the keyboard.
		if (document.querySelector('dialog[open]')) return;
		if (typing || event.metaKey || event.ctrlKey || event.altKey || !app.user) return;
		const place = places.find((p) => p.key === event.key);
		if (place) goto(place.href);
		else if (event.key === 'c') write();
		else if (event.key === '/') {
			event.preventDefault();
			goto('/search');
		} else if (event.key === 'm') menuOpen = !menuOpen;
	}
</script>

<svelte:window {onkeydown} />

{#if loadError}
	<p class="empty"><strong>emscreen is not responding</strong>{loadError}</p>
{:else if !app.ready}
	<p class="empty" aria-busy="true">Loading</p>
{:else if !app.user}
	<Login />
{:else}
	<header>
		<div class="bar column">
			<button class="btn primary" onclick={write}>Write</button>
			<button class="place" onclick={() => (menuOpen = !menuOpen)} aria-expanded={menuOpen} aria-controls="places">
				{here}
				<svg width="12" height="8" viewBox="0 0 12 8" aria-hidden="true"
					><path d="M1 1.5 6 6.5l5-5" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" /></svg
				>
			</button>
			<a class="btn quiet" href="/search">Search</a>
		</div>
		{#if menuOpen}
			<button class="scrim" aria-label="Close menu" onclick={() => (menuOpen = false)}></button>
			<nav id="places" class="column">
				<ul class="sheet">
					{#each places as place}
						<li>
							<a
								href={place.href}
								aria-current={place.href === page.url.pathname ? 'page' : undefined}
								onclick={() => (menuOpen = false)}
							>
								<span class="label">{place.label}</span>
								<span class="hint">{place.hint}</span>
								{#if place.badge}<span class="badge">{place.badge}</span>{/if}
								{#if place.key}<kbd>{place.key}</kbd>{/if}
							</a>
						</li>
					{/each}
					<li class="foot">
						<span class="muted">{app.user.email}</span>
						<button class="btn small" onclick={logout}>Sign out</button>
					</li>
				</ul>
			</nav>
		{/if}
	</header>
	<main class="column">
		{@render children()}
	</main>
{/if}

<style>
	header {
		position: sticky;
		top: 0;
		z-index: 10;
		background: color-mix(in srgb, var(--paper) 88%, transparent);
		backdrop-filter: blur(10px);
		border-bottom: 1px solid var(--line);
	}
	.bar {
		display: grid;
		grid-template-columns: 1fr auto 1fr;
		align-items: center;
		padding: 0.6rem 0;
	}
	.bar > :first-child {
		justify-self: start;
	}
	.bar > :last-child {
		justify-self: end;
	}
	.place {
		display: inline-flex;
		align-items: center;
		gap: 0.5rem;
		border: 0;
		background: none;
		font: 700 1.2rem var(--display);
		letter-spacing: -0.02em;
		padding: 0.3rem 0.8rem;
		border-radius: 999px;
		cursor: pointer;
	}
	.place:hover {
		background: var(--surface);
	}
	.scrim {
		position: fixed;
		inset: 0;
		border: 0;
		background: color-mix(in srgb, var(--ink) 25%, transparent);
		z-index: -1;
		top: 3.4rem;
		height: 100vh;
	}
	nav {
		position: absolute;
		left: 0;
		right: 0;
	}
	nav ul {
		list-style: none;
		margin: 0.5rem auto 0;
		padding: 0.4rem;
		max-width: 26rem;
		box-shadow: 0 18px 40px -18px color-mix(in srgb, var(--ink) 55%, transparent);
	}
	nav a {
		display: grid;
		grid-template-columns: 1fr auto auto;
		column-gap: 0.6rem;
		align-items: center;
		padding: 0.55rem 0.8rem;
		border-radius: 10px;
		text-decoration: none;
	}
	nav a:hover,
	nav a[aria-current='page'] {
		background: var(--paper);
	}
	.label {
		font-weight: 700;
	}
	.hint {
		grid-column: 1;
		grid-row: 2;
		font-size: 0.85rem;
		color: var(--ink-soft);
	}
	.hint:empty {
		display: none;
	}
	.badge {
		grid-column: 2;
		grid-row: 1;
		background: var(--signal);
		color: var(--signal-ink);
		font-weight: 700;
		font-size: 0.8rem;
		border-radius: 999px;
		padding: 0 0.5rem;
	}
	kbd {
		grid-column: 3;
		grid-row: 1;
		font: 600 0.75rem var(--body);
		color: var(--ink-soft);
		border: 1px solid var(--line);
		border-radius: 5px;
		padding: 0 0.35rem;
	}
	.foot {
		display: flex;
		justify-content: space-between;
		align-items: center;
		gap: 1rem;
		padding: 0.7rem 0.8rem 0.4rem;
		margin-top: 0.4rem;
		border-top: 1px solid var(--line);
		font-size: 0.9rem;
	}
	main {
		padding-bottom: 5rem;
	}
</style>
