package com.wasimaster.wmpass.app

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

/** The counting behind [LaunchGuard]: which ends of a process count as a crash, and when the look goes. */
class LaunchMarksTest {
    /** A new process: the marks as the last one left them, and whether the look was reset. */
    private fun launch(left: LaunchMarks): Pair<LaunchMarks, Boolean> = left.started()

    /** Door, then the vault, and the process dies before either has lasted. */
    private fun diesOnVault(start: LaunchMarks) = start.armed().settled().armed()

    @Test
    fun twoLaunchesInARowThatDieRightAfterDrawingResetTheLook() {
        val (first, firstReset) = launch(LaunchMarks())
        assertFalse(firstReset)
        val (second, secondReset) = launch(diesOnVault(first))
        assertFalse(secondReset)
        assertEquals(1, second.strikes)
        val (third, thirdReset) = launch(diesOnVault(second))
        assertTrue(thirdReset)
        assertEquals(LaunchMarks(notice = true), third)
    }

    @Test
    fun aLaunchThatSettlesForgivesTheCount() {
        val (afterCrash, _) = launch(diesOnVault(LaunchMarks()))
        assertEquals(1, afterCrash.strikes)
        val (afterGood, _) = launch(afterCrash.armed().settled())
        assertEquals(0, afterGood.strikes)
        val (afterNext, reset) = launch(diesOnVault(afterGood))
        assertFalse(reset)
        assertEquals(1, afterNext.strikes)
    }

    @Test
    fun leavingTheScreenBeforeTheEndNeverCounts() {
        // Swiped from recents, locked by the screen going off, or killed in the background:
        // each leaves the screen first, which settles the launch.
        var marks = LaunchMarks()
        repeat(5) {
            val (next, reset) = launch(marks.armed().settled())
            assertFalse(reset)
            assertEquals(0, next.strikes)
            marks = next
        }
    }

    @Test
    fun drawingAgainInOneLaunchStillCountsOnce() {
        val left = LaunchMarks().armed().settled().armed().armed()
        val (next, reset) = launch(left)
        assertFalse(reset)
        assertEquals(1, next.strikes)
    }

    @Test
    fun aProcessThatNeverDrewNeitherCountsNorForgives() {
        // Started for autofill or the keyboard, and ended without showing the door.
        val (afterCrash, _) = launch(diesOnVault(LaunchMarks()))
        val (afterAutofill, _) = launch(afterCrash)
        assertEquals(1, afterAutofill.strikes)
        val (_, reset) = launch(diesOnVault(afterAutofill))
        assertTrue(reset)
    }

    @Test
    fun theNoticeWaitsUntilItIsShown() {
        val (afterReset, _) = launch(diesOnVault(launch(diesOnVault(LaunchMarks())).first))
        assertTrue(afterReset.notice)
        val (later, reset) = launch(afterReset.armed().settled())
        assertFalse(reset)
        assertTrue(later.notice)
    }
}
