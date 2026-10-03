import { mailAction, type Target } from './swipe.ts';
import { notify } from './app.svelte.ts';

/** Marks a drag as carrying conversations from one of the lists. */
export const DRAG_TYPE = 'application/x-emscreen-mail';

export interface Dragged extends Target {
	/** Page the drag started on; decides what dropping on Inbox undoes. */
	from: string;
}

export const carriesMail = (event: DragEvent) => event.dataTransfer?.types.includes(DRAG_TYPE) ?? false;

export function startDrag(event: DragEvent, dragged: Dragged, image?: Element) {
	if (!event.dataTransfer) return;
	event.dataTransfer.setData(DRAG_TYPE, JSON.stringify(dragged));
	event.dataTransfer.effectAllowed = 'move';
	if (image) event.dataTransfer.setDragImage(image, 24, 24);
}

/** What dropping mail on a side bar entry does, by the entry's address. */
export const dropTargets: Record<string, string> = {
	'/': 'Move to Inbox',
	'/important': 'Move to Important',
	'/archive': 'Archive'
};

export async function dropOn(href: string, event: DragEvent) {
	const raw = event.dataTransfer?.getData(DRAG_TYPE);
	if (!raw) return;
	const dragged: Dragged = JSON.parse(raw);
	const count = (dragged.threadIds?.length ?? 0) + (dragged.messageIds?.length ?? 0);
	const what = `${count} ${dragged.threadIds?.length ? 'conversation' : 'mail'}${count === 1 ? '' : 's'}`;
	try {
		if (href === '/important') await mailAction('important', dragged, `Moved to Important: ${what}`);
		else if (href === '/archive') await mailAction('archive', dragged, `Archived: ${what}`);
		else if (href === '/' && dragged.from === '/delayed') await mailAction('undelay', dragged, `Back in the Inbox: ${what}`);
		else if (href === '/') await mailAction('unimportant', dragged, `Moved to Inbox: ${what}`);
	} catch (e) {
		notify((e as Error).message);
	}
}
