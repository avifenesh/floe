export type CreateUserInput = {
  email: string;
};

export async function createUser(input: CreateUserInput): Promise<{ id: string }> {
  return { id: input.email };
}
