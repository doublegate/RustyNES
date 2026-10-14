package com.doublegate.rustynes

/**
 * Which controller may write each SHA-keyed save file (v3.2.0, used by
 * [FdsSaves]).
 *
 * An FDS disk is flushed from three places -- the game loop once a second, the
 * lifecycle flush (onPause/onStop, close), and the outgoing flush on a ROM
 * switch -- on three different threads, and each one names its controller and
 * its key separately. Serializing the writes was not enough (review on #604):
 *
 * - the game loop captured its controller at the top of an iteration and read
 *   the ROM identity later, so a switch in between paired the OUTGOING disk
 *   with the INCOMING game's file (Cursor);
 * - a flush queued by the outgoing controller could run after the incoming one
 *   had written the same file, when the same disk was reopened, and put the
 *   older image back (CodeRabbit).
 *
 * Both are writes by a controller that no longer owns the key. Here a key has
 * at most one owner, a write by anyone else is refused, and a ROM switch is one
 * locked step: the outgoing controller's final flush, then the incoming claim.
 * A refused write leaves the controller's dirty flag set (the caller clears it
 * only inside [write]), so nothing is lost by refusing a write that was in
 * fact the incoming owner's, made before its claim landed: the next flush
 * retries it.
 *
 * Identity, not equality: owners are compared with `===`, because two
 * controllers for the same disk are equal in every way that matters except
 * which one the game loop is running.
 */
internal class SaveOwners<C : Any> {
    private val owners = HashMap<String, C>()

    /** Run [block] only if [ctrl] owns [key]; true when it ran. */
    @Synchronized
    fun write(key: String, ctrl: C, block: () -> Unit): Boolean {
        if (owners[key] !== ctrl) return false
        block()
        return true
    }

    /**
     * Retire [outCtrl] from [outKey] (running [finalFlush] first if it still
     * owns it) and give [inKey] to [inCtrl], with no write from either side
     * able to land in between. Either side may be absent: the first load has
     * nothing outgoing.
     */
    @Synchronized
    fun handOver(outKey: String?, outCtrl: C?, inKey: String?, inCtrl: C?, finalFlush: () -> Unit) {
        if (outKey != null && outCtrl != null && owners[outKey] === outCtrl) {
            finalFlush()
            owners.remove(outKey)
        }
        if (inKey != null && inCtrl != null) owners[inKey] = inCtrl
    }

    /** End [ctrl]'s ownership of [key]; a release by a former owner is ignored. */
    @Synchronized
    fun release(key: String, ctrl: C) {
        if (owners[key] === ctrl) owners.remove(key)
    }
}
