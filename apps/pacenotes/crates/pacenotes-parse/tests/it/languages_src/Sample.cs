using System;
using System.Collections.Generic;

namespace Sample
{
    public class Account
    {
        private int balance;
        public int Id { get; set; }

        public Account(int id)
        {
            Id = id;
        }

        public int Deposit(int n)
        {
            balance += n;
            return Math.Max(balance, 0);
        }
    }

    public interface IGreeter
    {
        void Greet(string name);
    }

    public enum Mode { Fast, Slow }

    public record Pair(int A, int B);
}
