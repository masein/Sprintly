import Link from "next/link";
import { AuthForm } from "@/components/AuthForm";

export const metadata = { title: "Create account · Sprintly" };

export default function RegisterPage() {
  return (
    <main className="mx-auto flex min-h-screen max-w-md flex-col justify-center gap-8 px-6 py-20">
      <header className="space-y-2">
        <div className="mono text-xs uppercase tracking-widest text-chrome-dim">
          sprintly · auth
        </div>
        <h1 className="text-3xl font-semibold">Create your account.</h1>
        <p className="text-sm text-chrome-dim">
          First user becomes admin. After that, registration needs an invite
          token unless your admin enabled open signup.
        </p>
      </header>

      <AuthForm mode="register" />

      <footer className="mono text-xs text-chrome-dim">
        already in?{" "}
        <Link href="/login" className="text-accent hover:underline">
          sign in
        </Link>
      </footer>
    </main>
  );
}
