package sample

import kotlin.math.max
import java.util.List

class Account(val id: Int) {
    var balance: Int = 0

    constructor() : this(0)

    fun deposit(n: Int): Int {
        balance += n
        return max(balance, 0)
    }
}

object Registry {
    fun find(id: Int): Account = Account(id)
}

interface Greeter {
    fun greet(name: String)
}

enum class Mode { FAST, SLOW }

fun topLevel(x: Int): Int = x + 1

fun String.shout(): String = this.uppercase()

suspend fun fetch(id: Int): Int = id

class Holder {
    companion object {
        const val LIMIT = 3
        fun create(): Holder = Holder()
    }
}

fun page() {
    html {
        body {
            text("hi")
        }
    }
}
