package com.doublegate.rustynes

/**
 * Which controller may write each SHA-keyed save file (v3.2.0, used by
 * [FdsSaves]).
 *
 * An FDS disk is flushed from three places -- the game loop once a second, the
 * lifecycle flush (onPause/onStop, close), and the outgoing flush on a ROM
 * switch -- on three different threads, and each one names its controller and
 * its key separately. Serializing the writes was not enough; review on #604
 * found three ways a write went to the wrong file or was lost:
 *
 * - the game loop captured its controller at the top of an iteration and read
 *   the ROM identity later, so a switch in between paired the OUTGOING disk
 *   with the INCOMING game's file (Cursor);
 * - a flush queued by the outgoing controller could run after the incoming
 *   one had written the same file, when the same disk was reopened, and put
 *   the older image back (CodeRabbit);
 * - this class's first version claimed the key later, on an I/O thread, so a
 *   close or a pause before the claim was refused and never retried, and two
 *   quick switches could finish out of order and leave a retired controller
 *   owning the key, after which the live session could not save (Cursor).
 *
 * So ownership moves in [transfer], which does no I/O and is called on the
 * main thread in the same step that swaps the running controller: there is no
 * window in which the running controller does not own its key. The outgoing
 * controller keeps a RETIRING right to its old key, so its final flush can run
 * later on an I/O thread, and loses it the moment the new owner first writes
 * that key: an older image can never replace a newer one. [finish] drops the
 * retiring right once the final flush has run, so the map does not keep a
 * finished controller (and its native memory) alive.
 *
 * Identity, not equality: controllers are compared with `===`, because two
 * controllers for the same disk are equal in every way that matters except
 * which one the game loop is running.
 */
internal class SaveOwners<C : Any> {
    private val owners = HashMap<String, C>()
    private val retiring = HashMap<String, C>()

    /**
     * Run [block] if [ctrl] owns [key], or is retiring from it and its owner
     * has not written it since; true when it ran. A write by the owner ends
     * any retiring right to the key.
     */
    @Synchronized
    fun write(key: String, ctrl: C, block: () -> Unit): Boolean {
        when {
            owners[key] === ctrl -> {
                block()
                retiring.remove(key)
            }
            retiring[key] === ctrl -> block()
            else -> return false
        }
        return true
    }

    /**
     * Move [outCtrl] off [outKey] (to a retiring right, if it owned it) and
     * give [inKey] to [inCtrl]. No I/O: call it where the running controller
     * is swapped. Either side may be absent; the first load has nothing
     * outgoing, and a close has nothing incoming.
     */
    @Synchronized
    fun transfer(outKey: String?, outCtrl: C?, inKey: String?, inCtrl: C?) {
        if (outKey != null && outCtrl != null && owners[outKey] === outCtrl) {
            owners.remove(outKey)
            retiring[outKey] = outCtrl
        }
        if (inKey != null && inCtrl != null) owners[inKey] = inCtrl
    }

    /** The final flush of a retired [ctrl] has run: drop its right to [key]. */
    @Synchronized
    fun finish(key: String, ctrl: C) {
        if (retiring[key] === ctrl) retiring.remove(key)
    }

    /**
     * Closing [ctrl]'s session: give up [key], run [finalFlush] if it owned it,
     * and keep nothing. One locked step. A close by a former owner is ignored.
     */
    @Synchronized
    fun close(key: String, ctrl: C, finalFlush: () -> Unit) {
        if (owners[key] !== ctrl) return
        owners.remove(key)
        finalFlush()
    }
}
